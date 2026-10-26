package com.crosbic.phonemedia

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONObject

class Pairings(context: Context) {
    private val prefs = context.getSharedPreferences("pairings", Context.MODE_PRIVATE)

    val phoneId: String = prefs.getString(PHONE_ID, null)
        ?: Crypto.hex(Crypto.random(16)).also { prefs.edit().putString(PHONE_ID, it).apply() }

    fun keyFor(pcId: String): ByteArray? =
        prefs.getString(PC + pcId, null)?.let { Crypto.fromBase64(JSONObject(it).getString("key")) }

    fun add(pcId: String, name: String, key: ByteArray) {
        val entry = JSONObject().put("name", name).put("key", Crypto.toBase64(key))
        prefs.edit().putString(PC + pcId, entry.toString()).apply()
    }

    fun names(): List<String> =
        prefs.all.filterKeys { it.startsWith(PC) }.values.map { JSONObject(it as String).getString("name") }.sorted()

    fun forgetAll() {
        val editor = prefs.edit()
        prefs.all.keys.filter { it.startsWith(PC) }.forEach(editor::remove)
        editor.apply()
    }

    private companion object {
        const val PHONE_ID = "phoneId"
        const val PC = "pc."
    }
}

object PairingWindow {
    @Volatile var isOpen = false

    var request: Pair<String, String>? = null
        private set
    var onChange: (() -> Unit)? = null

    private val main = Handler(Looper.getMainLooper())

    fun show(pcName: String, code: String) {
        main.post {
            val pretty = code.chunked(3).joinToString(" ")
            request = pcName to pretty
            MediaLog.add("ПК «$pcName» просит сопряжения, код $pretty")
            onChange?.invoke()
        }
    }

    fun hide() {
        main.post {
            request = null
            onChange?.invoke()
        }
    }
}
