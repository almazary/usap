package com.almazari.usap

import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Pembuat paket protokol Usap. Format lengkap ada di agent/src/proto.rs. */
object Proto {
    const val HELLO: Byte = 0x01
    const val TOUCH: Byte = 0x02
    const val PING: Byte = 0x03
    const val DISCOVER: Byte = 0x04
    const val BYE: Byte = 0x05
    const val NOTE: Byte = 0x06
    const val NOTE_CANCEL: Byte = 1
    const val BATCH: Byte = 0x07
    const val WELCOME = 0x81
    const val HAPTIC = 0x82
    const val PONG = 0x83
    const val HERE = 0x84

    /** Hasil hello: 1 = diterima, 0 = kode salah, 2 = dikunci sementara, -1 = tidak ada jawaban. */
    const val NO_ANSWER = -1

    fun hello(code: String, phoneName: String): ByteArray {
        val name = phoneName.toByteArray(Charsets.UTF_8).take(60).toByteArray()
        return ByteBuffer.allocate(8 + name.size)
            .put(HELLO).put(code.toByteArray(Charsets.US_ASCII)).put(name.size.toByte()).put(name).array()
    }

    fun touch(seq: Int, timeMs: Long, n: Int, ids: IntArray, xs: FloatArray, ys: FloatArray): ByteArray {
        val b = ByteBuffer.allocate(10 + n * 9).order(ByteOrder.LITTLE_ENDIAN)
        b.put(TOUCH).putInt(seq).putInt(timeMs.toInt()).put(n.toByte())
        for (i in 0 until n) b.put(ids[i].toByte()).putFloat(xs[i]).putFloat(ys[i])
        return b.array()
    }
}
