package com.crosbic.phonemedia

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONObject

class Pairings(context: Context) {
    private val prefs = context.getSharedPreferences("pairings", Context.MODE_PRIVATE)
    private val main = Handler(Looper.getMainLooper())

    val phoneId: String = prefs.getString(PHONE_ID, null)
        ?: Crypto.hex(Crypto.random(16)).also { prefs.edit().putString(PHONE_ID, it).apply() }

    fun keyFor(pcId: String): ByteArray? =
        prefs.getString(PC + pcId, null)?.let { Crypto.fromBase64(JSONObject(it).getString("key")) }

    fun add(pcId: String, name: String, key: ByteArray) {
        val entry = JSONObject().put("name", name).put("key", Crypto.toBase64(key))
        prefs.edit().putString(PC + pcId, entry.toString()).apply()
        publish()
    }

    fun forget(pcId: String) {
        prefs.edit().remove(PC + pcId).apply()
        publish()
    }

    fun list(): List<Pc> =
        prefs.all.filterKeys { it.startsWith(PC) }
            .map { (key, value) -> Pc(key.removePrefix(PC), JSONObject(value as String).getString("name")) }
            .sortedBy { it.name }

    fun publish() {
        val pcs = list()
        main.post { AppState.pairedPcs = pcs }
    }

    private companion object {
        const val PHONE_ID = "phoneId"
        const val PC = "pc."
    }
}

object PairingWindow {
    @Volatile var isOpen = false

    private val main = Handler(Looper.getMainLooper())

    fun show(pcName: String, code: String) {
        main.post {
            val pretty = code.chunked(3).joinToString(" ")
            AppState.pairingRequest = PairingRequest(pcName, pretty)
            MediaLog.add("ПК «$pcName» просит сопряжения, код $pretty")
        }
    }

    fun hide() {
        main.post { AppState.pairingRequest = null }
    }
}
