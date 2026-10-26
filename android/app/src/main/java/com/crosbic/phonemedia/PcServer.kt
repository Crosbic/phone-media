package com.crosbic.phonemedia

import android.os.Handler
import android.os.Looper
import org.json.JSONException
import org.json.JSONObject
import java.io.IOException
import java.io.Writer
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

class PcServer(
    private val deviceName: String,
    private val pairings: Pairings,
    private val onConnected: () -> Unit,
    private val onCommand: (JSONObject) -> Unit,
) {
    private class Connection(val socket: Socket, val writer: Writer)

    private val main = Handler(Looper.getMainLooper())
    private val sender = Executors.newSingleThreadScheduledExecutor()
    @Volatile private var running = true
    @Volatile private var udp: DatagramSocket? = null
    @Volatile private var server: ServerSocket? = null
    private val sockets = CopyOnWriteArraySet<Socket>()
    private val clients = ConcurrentHashMap<Socket, Pc>()
    private val connections = mutableListOf<Connection>()

    init {
        active = this
        thread(name = "pc-discovery") { guarded("поиск") { answerDiscovery() } }
        thread(name = "pc-accept") { guarded("сервер") { acceptClients() } }
        sender.scheduleWithFixedDelay({ write(PING) }, 10, 10, TimeUnit.SECONDS)
    }

    val isConnected get() = clients.isNotEmpty()

    fun send(message: JSONObject) {
        val line = message.toString()
        sender.execute { write(line) }
    }

    fun disconnect(pcId: String) {
        clients.filterValues { it.id == pcId }.keys.forEach { it.close() }
    }

    fun stop() {
        running = false
        if (active === this) active = null
        udp?.close()
        server?.close()
        sockets.forEach { it.close() }
        sender.shutdownNow()
        main.post { AppState.connectedPcs = emptyList() }
    }

    private fun answerDiscovery() {
        DatagramSocket(null).use { socket ->
            udp = socket
            socket.reuseAddress = true
            socket.bind(InetSocketAddress(DISCOVERY_PORT))
            val buffer = ByteArray(256)
            while (running) {
                val packet = DatagramPacket(buffer, buffer.size)
                socket.receive(packet)
                if (String(packet.data, 0, packet.length, Charsets.UTF_8) != QUERY) continue
                val answer = "$ANSWER $TCP_PORT ${pairings.phoneId} $deviceName".toByteArray(Charsets.UTF_8)
                socket.send(DatagramPacket(answer, answer.size, packet.socketAddress))
            }
        }
    }

    private fun acceptClients() {
        ServerSocket().use { socket ->
            server = socket
            socket.reuseAddress = true
            socket.bind(InetSocketAddress(TCP_PORT))
            log("жду ПК: UDP $DISCOVERY_PORT, TCP $TCP_PORT")
            while (running) {
                val pc = socket.accept()
                sockets.add(pc)
                thread(name = "pc-connection") { serve(pc) }
            }
        }
    }

    private fun serve(pc: Socket) {
        val address = pc.inetAddress.hostAddress
        try {
            pc.tcpNoDelay = true
            pc.soTimeout = HANDSHAKE_TIMEOUT_MS
            val reader = pc.getInputStream().bufferedReader()
            val writer = pc.getOutputStream().bufferedWriter()
            val handshake = Handshake(pairings, reader, writer)
            if (!handshake.run()) return
            pc.soTimeout = 0

            clients[pc] = Pc(handshake.pcId, handshake.pcName)
            publishConnected()
            sender.execute { connections.add(Connection(pc, writer)) }
            main.post {
                MediaLog.add("ПК подключился: ${handshake.pcName} ($address)")
                onConnected()
            }
            reader.forEachLine { line ->
                val command = try {
                    JSONObject(line)
                } catch (_: JSONException) {
                    null
                }
                if (command != null) main.post { onCommand(command) }
            }
        } catch (_: Exception) {
        } finally {
            pc.close()
            sockets.remove(pc)
            if (running) sender.execute { connections.removeAll { it.socket === pc } }
            if (clients.remove(pc) != null) {
                log("ПК отключился: $address")
                publishConnected()
            }
        }
    }

    private fun publishConnected() {
        val pcs = clients.values.distinctBy { it.id }.sortedBy { it.name }
        main.post { if (running) AppState.connectedPcs = pcs }
    }

    private fun write(line: String) {
        val iterator = connections.iterator()
        for (connection in iterator) {
            try {
                connection.writer.write(line)
                connection.writer.write("\n")
                connection.writer.flush()
            } catch (_: IOException) {
                connection.socket.close()
                iterator.remove()
            }
        }
    }

    private fun guarded(what: String, body: () -> Unit) {
        var lastError: String? = null
        while (running) {
            try {
                body()
            } catch (e: Exception) {
                if (!running) return
                val error = "$what: ошибка сети — ${e.message}"
                if (error != lastError) log(error)
                lastError = error
                Thread.sleep(5000)
            }
        }
    }

    private fun log(message: String) = MediaLog.post(message)

    companion object {
        const val DISCOVERY_PORT = 47800
        const val TCP_PORT = 47801
        private const val QUERY = "PHONEMEDIA? 2"
        private const val ANSWER = "PHONEMEDIA! 2"
        private const val HANDSHAKE_TIMEOUT_MS = 120_000
        private val PING = JSONObject().put("type", "ping").toString()

        @Volatile var active: PcServer? = null
            private set
    }
}
