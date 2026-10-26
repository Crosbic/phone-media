package com.crosbic.phonemedia

import org.json.JSONObject
import java.io.BufferedReader
import java.io.IOException
import java.io.Writer

class Handshake(
    private val pairings: Pairings,
    private val reader: BufferedReader,
    private val writer: Writer,
) {
    var pcName = "ПК"
        private set

    fun run(): Boolean {
        val hello = receive()
        if (hello.optString("type") != "hello") return false
        val pcId = hello.getString("pcId")
        pcName = hello.optString("name").ifEmpty { "ПК" }
        val pcNonce = hello.bytes("nonce")

        val key = pairings.keyFor(pcId)
        if (key != null) {
            val phoneNonce = Crypto.random(16)
            send(
                JSONObject().put("type", "auth").put("phoneId", pairings.phoneId)
                    .put("nonce", Crypto.toBase64(phoneNonce))
                    .put("proof", Crypto.toBase64(Crypto.hmac(key, "phone".toByteArray(), pcNonce, phoneNonce))),
            )
            val reply = receive()
            return when (reply.optString("type")) {
                "auth" -> Crypto.same(Crypto.hmac(key, "pc".toByteArray(), pcNonce, phoneNonce), reply.bytes("proof"))
                "pair" -> pair(pcId, reply)
                else -> false
            }
        }

        send(
            JSONObject().put("type", "unpaired").put("phoneId", pairings.phoneId)
                .put("canPair", PairingWindow.isOpen),
        )
        val request = receive()
        return request.optString("type") == "pair" && pair(pcId, request)
    }

    private fun pair(pcId: String, request: JSONObject): Boolean {
        if (!PairingWindow.isOpen) {
            send(JSONObject().put("type", "pairClosed"))
            return false
        }
        val commit = request.bytes("commit")
        val keys = Crypto.newKeyPair()
        val phoneKey = Crypto.encode(keys)
        send(JSONObject().put("type", "pairKey").put("key", Crypto.toBase64(phoneKey)))

        val reveal = receive()
        if (reveal.optString("type") != "pairReveal") return false
        val pcKey = reveal.bytes("key")
        if (!Crypto.same(Crypto.sha256(pcKey), commit)) return false

        val shared = Crypto.ecdh(keys, pcKey)
        val key = Crypto.hmac(shared, "phone-media key".toByteArray(), pcKey, phoneKey)
        PairingWindow.show(pcName, Crypto.code(shared, pcKey, phoneKey))
        try {
            val confirm = receive()
            if (confirm.optString("type") != "pairConfirm" ||
                !Crypto.same(Crypto.hmac(key, "confirm".toByteArray()), confirm.bytes("proof"))
            ) {
                MediaLog.post("ПК «$pcName»: код не подтверждён, сопряжение отменено")
                return false
            }
            pairings.add(pcId, pcName, key)
            send(JSONObject().put("type", "paired"))
            MediaLog.post("ПК «$pcName» сопряжён")
            return true
        } finally {
            PairingWindow.hide()
        }
    }

    private fun send(message: JSONObject) {
        writer.write(message.toString())
        writer.write("\n")
        writer.flush()
    }

    private fun receive() = JSONObject(reader.readLine() ?: throw IOException("ПК закрыл соединение"))

    private fun JSONObject.bytes(field: String) = Crypto.fromBase64(getString(field))
}
