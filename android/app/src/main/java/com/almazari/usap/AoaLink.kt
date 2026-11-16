package com.almazari.usap

import android.content.Context
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.os.Handler
import android.os.HandlerThread
import android.os.Process
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException

/**
 * Kabel USB tanpa mode developer (Android Open Accessory). usap-agent di Mac meminta HP masuk
 * mode aksesori "Usap"; aplikasi membuka aksesori itu dan bertukar paket lewat kabel,
 * setiap paket diawali panjang u16.
 */
class AoaLink(ctx: Context, accessory: UsbAccessory, code: String, phoneName: String) : Conn(code, phoneName) {
    override val label = " lewat kabel"
    override val usesWifi = false

    private val pfd = ctx.getSystemService(UsbManager::class.java).openAccessory(accessory)
        ?: throw IOException("aksesori tidak bisa dibuka")
    private val out = FileOutputStream(pfd.fileDescriptor)
    private val inp = FileInputStream(pfd.fileDescriptor)
    private val sendThread = HandlerThread("usap-aoa", Process.THREAD_PRIORITY_URGENT_DISPLAY).apply { start() }
    private val sender = Handler(sendThread.looper)
    @Volatile private var closed = false

    init {
        Thread({
            // sebagian HP gagal membaca aksesori kalau buffer lebih kecil dari 16 KB
            val buf = ByteArray(16384)
            var acc = ByteArray(0)
            try {
                while (!closed) {
                    val n = inp.read(buf)
                    if (n < 0) break
                    if (n == 0) continue
                    acc += buf.copyOf(n)
                    var i = 0
                    while (acc.size - i >= 2) {
                        val len = (acc[i].toInt() and 0xff) or ((acc[i + 1].toInt() and 0xff) shl 8)
                        if (len == 0 || len > 512) { i = acc.size; break }
                        if (acc.size - i < 2 + len) break
                        handle(acc.copyOfRange(i + 2, i + 2 + len), len)
                        i += 2 + len
                    }
                    acc = acc.copyOfRange(i, acc.size)
                }
            } catch (_: Exception) {
            }
            if (!closed) lost()
        }, "usap-aoa-recv").apply { isDaemon = true; start() }
    }

    override fun send(b: ByteArray) {
        sender.post {
            if (closed) return@post
            try {
                val f = ByteArray(b.size + 2)
                f[0] = (b.size and 0xff).toByte()
                f[1] = (b.size shr 8).toByte()
                System.arraycopy(b, 0, f, 2, b.size)
                out.write(f)
            } catch (_: IOException) {
                lost()
            }
        }
    }

    override fun close() {
        send(byteArrayOf(Proto.BYE))
        sender.post {
            closed = true
            try { pfd.close() } catch (_: Exception) {}
            sendThread.quitSafely()
        }
    }
}
