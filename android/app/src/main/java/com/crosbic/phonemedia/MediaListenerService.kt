package com.crosbic.phonemedia

import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Bitmap
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.media.MediaMetadata
import android.media.VolumeProvider
import android.media.session.MediaController
import android.media.session.MediaSession
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.service.notification.NotificationListenerService
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import kotlin.math.max
import kotlin.math.roundToInt

fun listenerComponent(context: Context) = ComponentName(context, MediaListenerService::class.java)

class MediaListenerService : NotificationListenerService() {
    private class Tracked(val controller: MediaController, val callback: MediaController.Callback) {
        var lastSummary = ""
    }

    private val tracked = mutableMapOf<MediaSession.Token, Tracked>()
    private var order = emptyList<MediaSession.Token>()
    private var mainToken: MediaSession.Token? = null
    private val sessionsListener = MediaSessionManager.OnActiveSessionsChangedListener { controllers ->
        track(controllers.orEmpty())
        scheduleSync()
    }

    private var pc: PcServer? = null
    private val main = Handler(Looper.getMainLooper())
    private val syncRunnable = Runnable {
        fine.probe(evenWhilePlaying = false, done = ::probeDone)
        activeSessions()?.let(::track)
        publishNowPlaying()
        syncPc()
    }
    private lateinit var fine: FineVolume
    private var reportedMax = 0
    private var reportedOutput = ""
    private val deviceCallback = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) = scheduleSync()
        override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) = scheduleSync()
    }
    private var pendingVolume: Int? = null
    private var sentPlayer: String? = null
    private var sentArt: Bitmap? = null
    private var artSent = false
    private val volumeReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) = scheduleSync()
    }

    override fun onListenerConnected() {
        pc?.stop()
        pc = PcServer(deviceName(), Pairings(this), onConnected = ::resendToPc, onCommand = ::execute)
        val manager = getSystemService(MediaSessionManager::class.java)
        val component = listenerComponent(this)
        manager.addOnActiveSessionsChangedListener(sessionsListener, component)
        MediaLog.add("доступ к уведомлениям есть, слежу за плеерами")
        val volumeChanges = IntentFilter().apply {
            addAction(VOLUME_CHANGED)
            addAction(MUTE_CHANGED)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(volumeReceiver, volumeChanges, RECEIVER_NOT_EXPORTED)
        } else {
            registerReceiver(volumeReceiver, volumeChanges)
        }
        fine = FineVolume(this, audio)
        MediaLog.add("громкость: ${fine.summary}")
        fine.probe(evenWhilePlaying = false, done = ::probeDone)
        audio.registerAudioDeviceCallback(deviceCallback, main)
        Updater.startPeriodic(this)
        track(manager.getActiveSessions(component))
        scheduleSync()
    }

    override fun onListenerDisconnected() {
        main.removeCallbacks(syncRunnable)
        runCatching { unregisterReceiver(volumeReceiver) }
        audio.unregisterAudioDeviceCallback(deviceCallback)
        Updater.stopPeriodic()
        pc?.stop()
        pc = null
        getSystemService(MediaSessionManager::class.java).removeOnActiveSessionsChangedListener(sessionsListener)
        tracked.values.forEach { it.controller.unregisterCallback(it.callback) }
        tracked.clear()
        order = emptyList()
        mainToken = null
        AppState.nowPlaying = null
        MediaLog.add("доступ к уведомлениям отключён")
    }

    private fun track(controllers: List<MediaController>) {
        order = controllers.map { it.sessionToken }
        val active = controllers.associateBy { it.sessionToken }
        var changed = false

        for (token in tracked.keys - active.keys) {
            val gone = tracked.remove(token) ?: continue
            gone.controller.unregisterCallback(gone.callback)
            MediaLog.add("${appName(gone.controller)}: плеер закрыт")
            changed = true
        }

        for ((token, controller) in active) {
            if (token in tracked) continue
            val callback = object : MediaController.Callback() {
                override fun onMetadataChanged(metadata: MediaMetadata?) = changed(token)
                override fun onPlaybackStateChanged(state: PlaybackState?) = changed(token)
                override fun onAudioInfoChanged(info: MediaController.PlaybackInfo) = scheduleSync()
            }
            controller.registerCallback(callback)
            tracked[token] = Tracked(controller, callback)
            report(token)
            changed = true
        }

        if (changed) MediaLog.add("активных плееров: ${active.size}")
        val first = order.firstOrNull { it in tracked }
        if (first != mainToken) {
            mainToken = first
            first?.let(tracked::get)?.let { MediaLog.add("главный плеер: ${appName(it.controller)}") }
        }
    }

    private fun activeSessions(): List<MediaController>? = try {
        getSystemService(MediaSessionManager::class.java).getActiveSessions(listenerComponent(this))
    } catch (_: SecurityException) {
        null
    }

    private fun probeDone() {
        val level = pendingVolume
        pendingVolume = null
        if (level != null) mainController()?.let { setVolume(it, level) }
        scheduleSync()
    }

    private fun changed(token: MediaSession.Token) {
        report(token)
        scheduleSync()
    }

    private fun scheduleSync() {
        main.removeCallbacks(syncRunnable)
        main.postDelayed(syncRunnable, SYNC_DELAY_MS)
    }

    private fun mainController() = order.firstNotNullOfOrNull { tracked[it]?.controller }

    private fun report(token: MediaSession.Token) {
        val entry = tracked[token] ?: return
        val controller = entry.controller
        val metadata = controller.metadata
        val state = controller.playbackState
        val art = artOf(metadata)

        val summary = "${appName(controller)}: ${artistOf(metadata).ifEmpty { "—" }} — " +
            "${metadata.text(MediaMetadata.METADATA_KEY_TITLE).ifEmpty { "—" }} [${stateName(state)}], " +
            "обложка: ${art?.let { "${it.width}×${it.height}" } ?: "нет"}, " +
            "кнопки: ${actionNames(state).joinToString(" ").ifEmpty { "нет" }}"
        if (summary == entry.lastSummary) return
        entry.lastSummary = summary
        MediaLog.add("$summary, ${mmss(position(state))} / ${mmss(durationOf(metadata))}")
    }

    private fun resendToPc() {
        sentPlayer = null
        sentArt = null
        artSent = false
        syncPc()
    }

    private fun publishNowPlaying() {
        val controller = mainController()
        AppState.nowPlaying = controller?.let {
            val metadata = it.metadata
            val state = it.playbackState?.state
            NowPlaying(
                app = appName(it),
                title = metadata.text(MediaMetadata.METADATA_KEY_TITLE),
                artist = artistOf(metadata),
                playing = state == PlaybackState.STATE_PLAYING || state == PlaybackState.STATE_BUFFERING,
                art = artOf(metadata),
            )
        }
    }

    private fun syncPc() {
        val pc = pc?.takeIf { it.isConnected } ?: return
        val controller = mainController()
        if (controller == null) {
            if (sentPlayer != IDLE) {
                pc.send(JSONObject().put("type", "idle"))
                sentPlayer = IDLE
                sentArt = null
                artSent = false
            }
            return
        }
        val metadata = controller.metadata
        val state = controller.playbackState

        val art = artOf(metadata)
        if (!artSent || !sameArt(art, sentArt)) {
            MediaLog.add(if (art != null) "на ПК: обложка" else "на ПК: без обложки")
            pc.send(artMessage(art))
            sentArt = art
            artSent = true
        }

        val player = JSONObject()
            .put("type", "player")
            .put("app", appName(controller))
            .put("title", metadata.text(MediaMetadata.METADATA_KEY_TITLE))
            .put("artist", artistOf(metadata))
            .put("album", metadata.text(MediaMetadata.METADATA_KEY_ALBUM))
            .put("playing", state?.state == PlaybackState.STATE_PLAYING || state?.state == PlaybackState.STATE_BUFFERING)
            .put("durationMs", durationOf(metadata))
            .put("actions", JSONArray(actionNames(state)))
        val volume = volumeOf(controller)
        player
            .put("volume", volume.level)
            .put("volumeMax", volume.max)
            .put("muted", volume.muted)
            .put("volumeFixed", volume.fixed)
        val key = "$player|${state?.position}|${state?.lastPositionUpdateTime}|${state?.playbackSpeed}"
        if (key == sentPlayer) return
        sentPlayer = key
        pc.send(player.put("positionMs", position(state)))
    }

    private fun execute(command: JSONObject) {
        if (command.optString("type") != "command") return
        val action = command.optString("action")
        val controller = mainController()
        if (controller == null) {
            MediaLog.add("команда с ПК: $action — но плеера нет")
            return
        }
        val controls = controller.transportControls
        when (action) {
            "volume" -> {
                val level = command.optInt("level")
                MediaLog.add("команда с ПК: громкость $level")
                if (streamOf(controller.playbackInfo) == AudioManager.STREAM_MUSIC) {
                    fine.probe(evenWhilePlaying = true, done = ::probeDone)
                }
                if (fine.probing) {
                    pendingVolume = level
                } else {
                    setVolume(controller, level)
                    scheduleSync()
                }
                return
            }
            "volumeUp" -> adjustVolume(controller, AudioManager.ADJUST_RAISE)
            "volumeDown" -> adjustVolume(controller, AudioManager.ADJUST_LOWER)
            "mute" -> adjustVolume(controller, AudioManager.ADJUST_TOGGLE_MUTE)
        }
        MediaLog.add("команда с ПК: $action")
        when (action) {
            "play" -> controls.play()
            "pause" -> controls.pause()
            "stop" -> controls.stop()
            "next" -> controls.skipToNext()
            "prev" -> controls.skipToPrevious()
            "seek" -> controls.seekTo(command.optLong("positionMs"))
        }
    }

    private class Volume(val level: Int, val max: Int, val muted: Boolean, val fixed: Boolean)

    private val audio: AudioManager get() = getSystemService(AudioManager::class.java)

    private fun isRemote(info: MediaController.PlaybackInfo?) =
        info?.playbackType == MediaController.PlaybackInfo.PLAYBACK_TYPE_REMOTE

    private fun streamOf(info: MediaController.PlaybackInfo?) =
        info?.audioAttributes?.volumeControlStream ?: AudioManager.STREAM_MUSIC

    private fun volumeOf(controller: MediaController): Volume {
        val info = controller.playbackInfo
        if (info != null && isRemote(info)) {
            return Volume(info.currentVolume, info.maxVolume, false, info.volumeControl == VolumeProvider.VOLUME_CONTROL_FIXED)
        }
        val stream = streamOf(info)
        val max = audio.getStreamMaxVolume(stream)
        val volume = if (fine.active(stream)) {
            Volume(fine.get(stream), max * FineVolume.STEPS, audio.isStreamMute(stream), audio.isVolumeFixed)
        } else {
            Volume(audio.getStreamVolume(stream), max, audio.isStreamMute(stream), audio.isVolumeFixed)
        }
        val output = fine.output()
        if (output != reportedOutput || volume.max != reportedMax) {
            MediaLog.add("громкость: $output, ${volume.max} шагов")
            reportedOutput = output
        }
        reportedMax = volume.max
        return volume
    }

    private fun setVolume(controller: MediaController, level: Int) {
        val info = controller.playbackInfo
        if (isRemote(info)) {
            controller.setVolumeTo(level, 0)
            return
        }
        val stream = streamOf(info)
        if (level > 0 && audio.isStreamMute(stream)) {
            audio.adjustStreamVolume(stream, AudioManager.ADJUST_UNMUTE, 0)
        }
        val max = audio.getStreamMaxVolume(stream)
        val fraction = if (reportedMax > 0) level.toFloat() / reportedMax else level.toFloat() / max
        if (fine.active(stream)) {
            fine.set(stream, (fraction * max * FineVolume.STEPS).roundToInt())
        } else {
            audio.setStreamVolume(stream, (fraction * max).roundToInt().coerceIn(0, max), 0)
        }
    }

    private fun adjustVolume(controller: MediaController, direction: Int) {
        val info = controller.playbackInfo
        if (isRemote(info)) {
            if (direction != AudioManager.ADJUST_TOGGLE_MUTE) controller.adjustVolume(direction, 0)
            return
        }
        audio.adjustStreamVolume(streamOf(info), direction, 0)
    }

    private fun appName(controller: MediaController): String = try {
        packageManager.getApplicationLabel(packageManager.getApplicationInfo(controller.packageName, 0)).toString()
    } catch (_: Exception) {
        controller.packageName
    }

    private fun deviceName(): String =
        Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME) ?: Build.MODEL

    private fun artMessage(art: Bitmap?): JSONObject {
        val message = JSONObject().put("type", "art")
        if (art == null) return message
        val scale = ART_MAX_SIDE.toFloat() / max(art.width, art.height)
        val image = if (scale < 1) {
            Bitmap.createScaledBitmap(art, (art.width * scale).roundToInt(), (art.height * scale).roundToInt(), true)
        } else {
            art
        }
        val jpeg = ByteArrayOutputStream().also { image.compress(Bitmap.CompressFormat.JPEG, 90, it) }.toByteArray()
        return message.put("jpeg", Base64.encodeToString(jpeg, Base64.NO_WRAP))
    }

    private companion object {
        const val VOLUME_CHANGED = "android.media.VOLUME_CHANGED_ACTION"
        const val MUTE_CHANGED = "android.media.STREAM_MUTE_CHANGED_ACTION"
        const val IDLE = "idle"
        const val ART_MAX_SIDE = 512
        const val SYNC_DELAY_MS = 100L
    }
}

private fun MediaMetadata?.text(key: String): String = this?.getString(key).orEmpty()

private fun artistOf(metadata: MediaMetadata?) =
    metadata.text(MediaMetadata.METADATA_KEY_ARTIST).ifEmpty { metadata.text(MediaMetadata.METADATA_KEY_ALBUM_ARTIST) }

private fun durationOf(metadata: MediaMetadata?) = metadata?.getLong(MediaMetadata.METADATA_KEY_DURATION) ?: 0

private fun artOf(metadata: MediaMetadata?): Bitmap? = metadata?.let {
    it.getBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART)
        ?: it.getBitmap(MediaMetadata.METADATA_KEY_ART)
        ?: it.getBitmap(MediaMetadata.METADATA_KEY_DISPLAY_ICON)
}

private fun sameArt(a: Bitmap?, b: Bitmap?) = if (a == null || b == null) a === b else a === b || a.sameAs(b)

private val STATE_NAMES = mapOf(
    PlaybackState.STATE_NONE to "нет",
    PlaybackState.STATE_STOPPED to "остановлено",
    PlaybackState.STATE_PAUSED to "пауза",
    PlaybackState.STATE_PLAYING to "играет",
    PlaybackState.STATE_FAST_FORWARDING to "перемотка вперёд",
    PlaybackState.STATE_REWINDING to "перемотка назад",
    PlaybackState.STATE_BUFFERING to "буферизация",
    PlaybackState.STATE_ERROR to "ошибка",
    PlaybackState.STATE_CONNECTING to "подключение",
    PlaybackState.STATE_SKIPPING_TO_PREVIOUS to "к предыдущему",
    PlaybackState.STATE_SKIPPING_TO_NEXT to "к следующему",
)

private val ACTION_NAMES = listOf(
    PlaybackState.ACTION_PLAY to "play",
    PlaybackState.ACTION_PAUSE to "pause",
    PlaybackState.ACTION_PLAY_PAUSE to "playPause",
    PlaybackState.ACTION_STOP to "stop",
    PlaybackState.ACTION_SKIP_TO_PREVIOUS to "prev",
    PlaybackState.ACTION_SKIP_TO_NEXT to "next",
    PlaybackState.ACTION_SEEK_TO to "seek",
)

private fun stateName(state: PlaybackState?) =
    state?.let { STATE_NAMES[it.state] ?: "код ${it.state}" } ?: "нет состояния"

private fun actionNames(state: PlaybackState?): List<String> {
    val actions = state?.actions ?: 0
    return ACTION_NAMES.filter { (flag, _) -> actions and flag != 0L }.map { it.second }
}

private fun position(state: PlaybackState?): Long {
    if (state == null || state.position < 0) return 0
    if (state.state != PlaybackState.STATE_PLAYING) return state.position
    val elapsed = SystemClock.elapsedRealtime() - state.lastPositionUpdateTime
    return state.position + (elapsed * state.playbackSpeed).toLong()
}

private fun mmss(millis: Long): String {
    val seconds = millis.coerceAtLeast(0) / 1000
    return "%d:%02d".format(seconds / 60, seconds % 60)
}
