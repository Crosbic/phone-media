package com.crosbic.phonemedia

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import kotlin.concurrent.thread

data class Release(val version: String, val url: String, val sha256: String?)

sealed interface UpdateState {
    data object Idle : UpdateState
    data object Checking : UpdateState
    data object UpToDate : UpdateState
    data class Available(val release: Release) : UpdateState
    data class Downloading(val release: Release, val progress: Float) : UpdateState
    data class Installing(val release: Release) : UpdateState
    data class Failed(val release: Release?, val message: String) : UpdateState
}

object Updater {
    private const val LATEST = "https://api.github.com/repos/Crosbic/phone-media/releases/latest"
    private const val CHECK_EVERY_MS = 6 * 60 * 60 * 1000L
    private const val CHECK_ON_OPEN_MS = 60 * 60 * 1000L
    private const val FIRST_CHECK_MS = 30 * 1000L

    private val main = Handler(Looper.getMainLooper())
    @Volatile private var lastCheck = 0L
    private var context: Context? = null
    private val periodic = Runnable { scheduledCheck() }

    fun startPeriodic(context: Context) {
        this.context = context.applicationContext
        main.removeCallbacks(periodic)
        main.postDelayed(periodic, FIRST_CHECK_MS)
    }

    fun stopPeriodic() {
        main.removeCallbacks(periodic)
    }

    fun checkOnOpen(context: Context) {
        if (lastCheck == 0L || SystemClock.elapsedRealtime() - lastCheck > CHECK_ON_OPEN_MS) check(context, manual = false)
    }

    fun currentVersion(context: Context): String =
        context.packageManager.getPackageInfo(context.packageName, 0).versionName ?: "?"

    fun check(context: Context, manual: Boolean) {
        val app = context.applicationContext
        lastCheck = SystemClock.elapsedRealtime()
        val busy = AppState.update is UpdateState.Downloading || AppState.update is UpdateState.Installing
        if (busy) return
        if (manual) AppState.update = UpdateState.Checking
        thread(name = "update-check") {
            val result = runCatching { latest(app) }
            main.post {
                if (AppState.update is UpdateState.Downloading || AppState.update is UpdateState.Installing) return@post
                result.onSuccess { release ->
                    when {
                        release != null -> {
                            if (AppState.update !is UpdateState.Available) MediaLog.add("доступна версия ${release.version}")
                            AppState.update = UpdateState.Available(release)
                        }
                        manual -> AppState.update = UpdateState.UpToDate
                        AppState.update is UpdateState.Available -> AppState.update = UpdateState.Idle
                    }
                }
                result.onFailure { error ->
                    if (manual) AppState.update = UpdateState.Failed(null, "Не удалось проверить обновления: ${error.message}")
                    MediaLog.add("проверка обновлений: ${error.message}")
                }
            }
        }
    }

    fun install(context: Context, release: Release) {
        val app = context.applicationContext
        AppState.update = UpdateState.Downloading(release, 0f)
        MediaLog.add("скачиваю версию ${release.version}")
        thread(name = "update-download") {
            try {
                val file = File(app.cacheDir, "update.apk")
                download(release.url, file) { progress ->
                    main.post { AppState.update = UpdateState.Downloading(release, progress) }
                }
                if (release.sha256 != null && Crypto.hex(sha256(file)) != release.sha256) {
                    throw IOException("файл повреждён при скачивании")
                }
                commit(app, file)
                main.post { AppState.update = UpdateState.Installing(release) }
            } catch (error: Exception) {
                main.post {
                    AppState.update = UpdateState.Failed(release, "Не удалось обновить: ${error.message}")
                    MediaLog.add("обновление: ${error.message}")
                }
            }
        }
    }

    fun failed(message: String) {
        val release = when (val state = AppState.update) {
            is UpdateState.Installing -> state.release
            is UpdateState.Downloading -> state.release
            is UpdateState.Available -> state.release
            else -> null
        }
        AppState.update = UpdateState.Failed(release, message)
        MediaLog.add("обновление: $message")
    }

    private fun scheduledCheck() {
        context?.let { check(it, manual = false) }
        main.postDelayed(periodic, CHECK_EVERY_MS)
    }

    private fun latest(context: Context): Release? {
        val json = JSONObject(String(request(latestUrl(context)).use { it.inputStream.readBytes() }))
        val version = json.getString("tag_name").removePrefix("v")
        if (parse(version) <= parse(currentVersion(context))) return null
        val name = "PhoneMedia-$version.apk"
        val assets = json.getJSONArray("assets")
        for (index in 0 until assets.length()) {
            val asset = assets.getJSONObject(index)
            if (asset.getString("name") == name) {
                val digest = asset.optString("digest").removePrefix("sha256:").ifEmpty { null }
                return Release(version, asset.getString("browser_download_url"), digest)
            }
        }
        return null
    }

    private fun latestUrl(context: Context): String {
        val info = context.packageManager.getApplicationInfo(context.packageName, PackageManager.GET_META_DATA)
        return info.metaData?.getString("com.crosbic.phonemedia.UPDATE_URL").orEmpty().ifEmpty { LATEST }
    }

    private fun download(url: String, file: File, progress: (Float) -> Unit) {
        request(url).use { connection ->
            val total = connection.contentLengthLong
            var loaded = 0L
            var reported = -1
            connection.inputStream.use { input ->
                file.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        output.write(buffer, 0, read)
                        loaded += read
                        if (total > 0) {
                            val percent = (loaded * 100 / total).toInt()
                            if (percent != reported) {
                                reported = percent
                                progress(percent / 100f)
                            }
                        }
                    }
                }
            }
        }
    }

    private fun request(url: String): Connection {
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.connectTimeout = 15_000
        connection.readTimeout = 30_000
        connection.setRequestProperty("User-Agent", "PhoneMedia")
        connection.setRequestProperty("Accept", "application/vnd.github+json")
        if (connection.responseCode !in 200..299) {
            connection.disconnect()
            throw IOException("HTTP ${connection.responseCode}")
        }
        return Connection(connection)
    }

    private class Connection(private val connection: HttpURLConnection) : AutoCloseable {
        val inputStream get() = connection.inputStream
        val contentLengthLong get() = connection.contentLengthLong
        override fun close() = connection.disconnect()
    }

    private fun commit(context: Context, file: File) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
            setAppPackageName(context.packageName)
            if (Build.VERSION.SDK_INT >= 31) setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
        }
        val id = installer.createSession(params)
        installer.openSession(id).use { session ->
            session.openWrite("PhoneMedia.apk", 0, file.length()).use { output ->
                file.inputStream().use { it.copyTo(output) }
                session.fsync(output)
            }
            val status = PendingIntent.getBroadcast(
                context,
                id,
                Intent(context, UpdateReceiver::class.java),
                PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            session.commit(status.intentSender)
        }
    }

    private fun sha256(file: File): ByteArray {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                digest.update(buffer, 0, read)
            }
        }
        return digest.digest()
    }

    private fun parse(version: String): Triple<Int, Int, Int> {
        val parts = version.split('.').map { it.toIntOrNull() ?: 0 }
        return Triple(parts.getOrElse(0) { 0 }, parts.getOrElse(1) { 0 }, parts.getOrElse(2) { 0 })
    }

    private operator fun Triple<Int, Int, Int>.compareTo(other: Triple<Int, Int, Int>): Int =
        compareValuesBy(this, other, { it.first }, { it.second }, { it.third })
}

class UpdateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                val confirm = if (Build.VERSION.SDK_INT >= 33) {
                    intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(Intent.EXTRA_INTENT)
                }
                confirm?.let { context.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
            }
            PackageInstaller.STATUS_SUCCESS -> Unit
            PackageInstaller.STATUS_FAILURE_ABORTED -> Updater.failed("Установка отменена")
            else -> Updater.failed(intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "Android не установил обновление")
        }
    }
}
