package com.almazari.usap

import android.os.SystemClock
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Sambungan ke usap-agent, apa pun jalurnya (Wi-Fi, kabel USB, Bluetooth). */
abstract class Conn(private val code: String, private val phoneName: String) {
    /** Tambahan untuk judul layar trackpad, misalnya " lewat USB". */
    abstract val label: String
    abstract val usesWifi: Boolean

    @Volatile var computerName = ""
        private set
    @Volatile var lastPong = 0L
        private set
    @Volatile var onHaptic: (() -> Unit)? = null
    /** Sambungan putus dari sisi HP (misalnya kabel dicabut). */
    @Volatile var onLost: (() -> Unit)? = null

    protected fun lost() {
        val f = onLost
        onLost = null
        f?.invoke()
    }

    @Volatile private var welcome: CountDownLatch? = null
    @Volatile private var welcomeResult = Proto.NO_ANSWER
    private var seq = 0

    protected abstract fun send(b: ByteArray)
    abstract fun close()

    /** Balasan dari agen. */
    protected fun handle(buf: ByteArray, n: Int) {
        if (n <= 0) return
        when (buf[0].toInt() and 0xff) {
            Proto.WELCOME -> {
                welcomeResult = if (n > 1) buf[1].toInt() else 0
                if (n > 2) computerName = String(buf, 2, n - 2, Charsets.UTF_8)
                lastPong = SystemClock.uptimeMillis()
                welcome?.countDown()
            }
            Proto.HAPTIC -> onHaptic?.invoke()
            Proto.PONG -> lastPong = SystemClock.uptimeMillis()
        }
    }

    protected fun helloPacket() = Proto.hello(code, phoneName)

    /** Kirim HELLO dan tunggu jawaban. Jangan dipanggil dari thread UI. */
    fun hello(): Int {
        val b = helloPacket()
        repeat(3) {
            val latch = CountDownLatch(1)
            welcomeResult = Proto.NO_ANSWER
            welcome = latch
            send(b)
            if (latch.await(1000, TimeUnit.MILLISECONDS)) return welcomeResult
        }
        return Proto.NO_ANSWER
    }

    /** Snapshot semua jari yang menempel. Posisi dalam milimeter. n = 0 berarti semua diangkat. */
    fun touch(timeMs: Long, n: Int, ids: IntArray, xs: FloatArray, ys: FloatArray) =
        send(Proto.touch(++seq, timeMs, n, ids, xs, ys))

    fun ping() = send(byteArrayOf(Proto.PING))

    /** Android membatalkan sentuhan (misalnya gestur kembali dari tepi layar). */
    fun cancelled() = send(byteArrayOf(Proto.NOTE, Proto.NOTE_CANCEL))
}
