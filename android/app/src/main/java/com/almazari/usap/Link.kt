package com.almazari.usap

import android.os.Handler
import android.os.HandlerThread
import android.os.Process
import java.io.DataInputStream
import java.io.OutputStream
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket

/**
 * Sambungan lewat Wi-Fi (UDP) atau kabel USB (TCP ke 127.0.0.1, diteruskan `adb reverse`;
 * setiap paket diawali panjang u16).
 */
class Link(host: InetAddress, port: Int, code: String, phoneName: String, val usb: Boolean = false) : Conn(code, phoneName) {
    companion object {
        const val PORT = 50505
    }

    override val label = if (usb) " lewat USB" else ""
    override val usesWifi = !usb

    private val sock: DatagramSocket? = if (usb) null else DatagramSocket().apply { connect(host, port) }
    private val tcp: Socket? = if (usb) Socket().apply {
        tcpNoDelay = true
        connect(InetSocketAddress(host, port), 1500)
    } else null
    private val tcpOut: OutputStream? = tcp?.getOutputStream()
    private val sendThread = HandlerThread("usap-send", Process.THREAD_PRIORITY_URGENT_DISPLAY).apply { start() }
    private val sender = Handler(sendThread.looper)
    @Volatile private var closed = false

    init {
        Thread({
            val buf = ByteArray(512)
            if (tcp != null) {
                val inp = DataInputStream(tcp.getInputStream())
                while (!closed) {
                    try {
                        val n = inp.readUnsignedByte() or (inp.readUnsignedByte() shl 8)
                        if (n == 0 || n > buf.size) break
                        inp.readFully(buf, 0, n)
                        handle(buf, n)
                    } catch (_: Exception) {
                        break
                    }
                }
            } else {
                val p = DatagramPacket(buf, buf.size)
                while (!closed) {
                    try {
                        sock!!.receive(p)
                    } catch (e: Exception) {
                        if (closed) break else continue
                    }
                    handle(buf, p.length)
                }
            }
        }, "usap-recv").apply { isDaemon = true; start() }
    }

    override fun send(b: ByteArray) {
        sender.post {
            try {
                if (tcpOut != null) {
                    val f = ByteArray(b.size + 2)
                    f[0] = (b.size and 0xff).toByte()
                    f[1] = (b.size shr 8).toByte()
                    System.arraycopy(b, 0, f, 2, b.size)
                    tcpOut.write(f)
                } else {
                    sock!!.send(DatagramPacket(b, b.size))
                }
            } catch (_: Exception) {
            }
        }
    }

    override fun close() {
        send(byteArrayOf(Proto.BYE))
        sender.post {
            closed = true
            try { sock?.close(); tcp?.close() } catch (_: Exception) {}
            sendThread.quitSafely()
        }
    }
}
