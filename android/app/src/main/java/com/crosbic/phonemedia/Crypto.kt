package com.crosbic.phonemedia

import android.util.Base64
import java.math.BigInteger
import java.nio.ByteBuffer
import java.security.KeyFactory
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec
import java.util.Locale
import javax.crypto.KeyAgreement
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

object Crypto {
    private val secureRandom = SecureRandom()

    fun random(size: Int) = ByteArray(size).also(secureRandom::nextBytes)

    fun sha256(data: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(data)

    fun hmac(key: ByteArray, vararg parts: ByteArray): ByteArray {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(key, "HmacSHA256"))
        parts.forEach(mac::update)
        return mac.doFinal()
    }

    fun same(a: ByteArray, b: ByteArray) = MessageDigest.isEqual(a, b)

    fun newKeyPair(): KeyPair =
        KeyPairGenerator.getInstance("EC").apply { initialize(ECGenParameterSpec("secp256r1")) }.generateKeyPair()

    fun encode(key: KeyPair): ByteArray {
        val point = (key.public as ECPublicKey).w
        return byteArrayOf(4) + point.affineX.toFixed(32) + point.affineY.toFixed(32)
    }

    fun ecdh(own: KeyPair, other: ByteArray): ByteArray {
        require(other.size == 65 && other[0] == 4.toByte()) { "ключ ПК не в формате SEC1" }
        val point = ECPoint(BigInteger(1, other.copyOfRange(1, 33)), BigInteger(1, other.copyOfRange(33, 65)))
        val spec = ECPublicKeySpec(point, (own.public as ECPublicKey).params)
        val public = KeyFactory.getInstance("EC").generatePublic(spec)
        return KeyAgreement.getInstance("ECDH").run {
            init(own.private)
            doPhase(public, true)
            generateSecret()
        }
    }

    fun code(shared: ByteArray, pcKey: ByteArray, phoneKey: ByteArray): String {
        val digest = hmac(shared, "phone-media code".toByteArray(), pcKey, phoneKey)
        val number = ByteBuffer.wrap(digest, 0, 4).int.toLong() and 0xffffffffL
        return String.format(Locale.ROOT, "%06d", number % 1_000_000)
    }

    fun toBase64(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.NO_WRAP)

    fun fromBase64(text: String): ByteArray = Base64.decode(text, Base64.NO_WRAP)

    fun hex(bytes: ByteArray) = bytes.joinToString("") { String.format(Locale.ROOT, "%02x", it) }

    private fun BigInteger.toFixed(size: Int): ByteArray {
        val bytes = toByteArray()
        return when {
            bytes.size == size -> bytes
            bytes.size > size -> bytes.copyOfRange(bytes.size - size, bytes.size)
            else -> ByteArray(size - bytes.size) + bytes
        }
    }
}
