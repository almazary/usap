package com.almazari.usap

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.ParcelUuid
import android.os.Process
import android.os.SystemClock
import java.io.ByteArrayOutputStream
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Sambungan lewat Bluetooth Low Energy. HP memancarkan layanan Usap; usap-agent di Mac mencari
 * dan menyambung ke HP. Paket ke agen dikirim lewat notifikasi TX, beberapa sekaligus dalam BATCH.
 * Balasan agen masuk lewat tulisan ke RX. Semua urusan Bluetooth berjalan di satu thread.
 */
@SuppressLint("MissingPermission")
class BleLink(private val ctx: Context, code: String, phoneName: String) : Conn(code, phoneName) {
    companion object {
        val SERVICE: UUID = UUID.fromString("7a1c0001-5e2a-4c6b-9a57-2f0d8b6c3e10")
        val TX: UUID = UUID.fromString("7a1c0002-5e2a-4c6b-9a57-2f0d8b6c3e10")
        val RX: UUID = UUID.fromString("7a1c0003-5e2a-4c6b-9a57-2f0d8b6c3e10")
        val CCC: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
    }

    override val label = " lewat Bluetooth"
    override val usesWifi = false

    private val thread = HandlerThread("usap-ble", Process.THREAD_PRIORITY_URGENT_DISPLAY).apply { start() }
    private val h = Handler(thread.looper)
    private val manager = ctx.getSystemService(BluetoothManager::class.java)
    private var server: BluetoothGattServer? = null
    private var txChar: BluetoothGattCharacteristic? = null
    private var central: BluetoothDevice? = null
    private var boost: BluetoothGatt? = null
    private var subscribed = false
    private var helloDone = false
    private var busySince = 0L
    private var mtu = 23
    private val pending = ArrayDeque<ByteArray>()
    private val ready = CountDownLatch(1)
    private var advertising = false
    private var withName = true
    @Volatile private var closed = false

    private val advCb = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
            advertising = true
        }

        override fun onStartFailure(errorCode: Int) {
            // nama HP terlalu panjang untuk paket iklan: coba lagi tanpa nama
            if (errorCode == ADVERTISE_FAILED_DATA_TOO_LARGE && withName) {
                withName = false
                h.post { advertise() }
            }
        }
    }

    /** Mulai memancarkan. Kembalikan pesan kesalahan, atau null kalau berhasil. */
    fun start(): String? {
        val adapter = manager?.adapter ?: return "HP ini tidak punya Bluetooth."
        if (!adapter.isEnabled) return "Nyalakan Bluetooth di HP dulu."
        if (adapter.bluetoothLeAdvertiser == null) return "HP ini tidak bisa memancarkan Bluetooth LE."
        val srv = manager.openGattServer(ctx, cb) ?: return "Gagal membuka layanan Bluetooth."
        server = srv
        val service = BluetoothGattService(SERVICE, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        val tx = BluetoothGattCharacteristic(TX, BluetoothGattCharacteristic.PROPERTY_NOTIFY, BluetoothGattCharacteristic.PERMISSION_READ)
        tx.addDescriptor(BluetoothGattDescriptor(CCC, BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE))
        val rx = BluetoothGattCharacteristic(
            RX,
            BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE or BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_WRITE,
        )
        service.addCharacteristic(tx)
        service.addCharacteristic(rx)
        txChar = tx
        if (!srv.addService(service)) return "Gagal menyiapkan layanan Bluetooth."
        return null
    }

    private fun advertise() {
        if (closed || advertising) return
        val adv = manager?.adapter?.bluetoothLeAdvertiser ?: return
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .setConnectable(true)
            .setTimeout(0)
            .build()
        val data = AdvertiseData.Builder().addServiceUuid(ParcelUuid(SERVICE)).setIncludeDeviceName(false).build()
        val scan = AdvertiseData.Builder().setIncludeDeviceName(withName).build()
        try {
            adv.startAdvertising(settings, data, scan, advCb)
        } catch (_: Exception) {
        }
    }

    private fun stopAdvertising() {
        if (!advertising) return
        advertising = false
        try {
            manager?.adapter?.bluetoothLeAdvertiser?.stopAdvertising(advCb)
        } catch (_: Exception) {
        }
    }

    /** Minta jeda sambungan sependek mungkin (sekitar 11–15 ms) supaya gerakan kursor rapat. */
    private fun boostConnection(device: BluetoothDevice) {
        try {
            boost?.close()
            boost = device.connectGatt(ctx, false, object : BluetoothGattCallback() {
                override fun onConnectionStateChange(g: BluetoothGatt, status: Int, newState: Int) {
                    if (newState == BluetoothProfile.STATE_CONNECTED) g.requestConnectionPriority(BluetoothGatt.CONNECTION_PRIORITY_HIGH)
                }
            }, BluetoothDevice.TRANSPORT_LE)
        } catch (_: Exception) {
        }
    }

    private val cb = object : BluetoothGattServerCallback() {
        override fun onServiceAdded(status: Int, service: BluetoothGattService) {
            h.post { advertise() }
        }

        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            h.post {
                if (newState == BluetoothProfile.STATE_CONNECTED) {
                    if (central == null) {
                        central = device
                        stopAdvertising()
                        boostConnection(device)
                    }
                } else if (device == central) {
                    central = null
                    subscribed = false
                    busySince = 0
                    pending.clear()
                    boost?.close()
                    boost = null
                    // agen dijalankan ulang atau Mac menjauh: pancarkan lagi supaya bisa disambung ulang
                    advertise()
                }
            }
        }

        override fun onMtuChanged(device: BluetoothDevice, newMtu: Int) {
            h.post { mtu = newMtu }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice, requestId: Int, descriptor: BluetoothGattDescriptor,
            preparedWrite: Boolean, responseNeeded: Boolean, offset: Int, value: ByteArray?,
        ) {
            if (responseNeeded) server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            if (descriptor.uuid != CCC) return
            h.post {
                subscribed = value != null && value.isNotEmpty() && (value[0].toInt() and 1) == 1
                if (subscribed) {
                    // sambung ulang setelah terputus: sapa agen lagi supaya tetap diterima
                    if (helloDone) pending.addFirst(helloPacket())
                    ready.countDown()
                    sendNext()
                }
            }
        }

        override fun onDescriptorReadRequest(device: BluetoothDevice, requestId: Int, offset: Int, descriptor: BluetoothGattDescriptor) {
            val v = if (subscribed) BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE else BluetoothGattDescriptor.DISABLE_NOTIFICATION_VALUE
            server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, v)
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice, requestId: Int, characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean, responseNeeded: Boolean, offset: Int, value: ByteArray?,
        ) {
            if (responseNeeded) server?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, 0, null)
            if (characteristic.uuid == RX && value != null) handle(value, value.size)
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            h.post {
                busySince = 0
                sendNext()
            }
        }
    }

    /** Kirim paket yang menunggu, sebanyak muat dalam satu notifikasi. Dijalankan di thread Bluetooth. */
    private fun sendNext() {
        val dev = central ?: return
        val ch = txChar ?: return
        val srv = server ?: return
        if (!subscribed || pending.isEmpty()) return
        // notifikasi sebelumnya belum dikonfirmasi; kalau macet lebih dari 300 ms anggap hilang
        if (busySince != 0L && SystemClock.uptimeMillis() - busySince < 300) return
        val max = (mtu - 3).coerceAtLeast(20)
        val taken = ArrayList<ByteArray>()
        val out = ByteArrayOutputStream()
        out.write(Proto.BATCH.toInt())
        while (pending.isNotEmpty() && out.size() + 1 + pending.first().size <= max) {
            val p = pending.removeFirst()
            out.write(p.size)
            out.write(p)
            taken.add(p)
        }
        val payload = if (taken.isEmpty()) {
            // satu paket pun tidak muat dengan pembungkus BATCH: kirim apa adanya kalau masih muat
            val p = pending.removeFirst()
            if (p.size > max) { h.post { sendNext() }; return }
            taken.add(p)
            p
        } else out.toByteArray()
        val ok = try {
            if (Build.VERSION.SDK_INT >= 33) {
                srv.notifyCharacteristicChanged(dev, ch, false, payload) == BluetoothStatusCodes.SUCCESS
            } else {
                @Suppress("DEPRECATION")
                ch.value = payload
                @Suppress("DEPRECATION")
                srv.notifyCharacteristicChanged(dev, ch, false)
            }
        } catch (_: Exception) {
            false
        }
        if (ok) {
            busySince = SystemClock.uptimeMillis()
        } else {
            for (p in taken.asReversed()) pending.addFirst(p)
            h.postDelayed({ sendNext() }, 4)
        }
    }

    override fun send(b: ByteArray) {
        h.post {
            if (b.isNotEmpty() && b[0] == Proto.HELLO) helloDone = true
            pending.addLast(b)
            // sambungan macet lama: buang yang paling lama supaya tidak menumpuk
            while (pending.size > 300) pending.removeFirst()
            sendNext()
        }
    }

    /** Tunggu sampai agen menyambung dan berlangganan notifikasi. */
    fun awaitReady(ms: Long): Boolean = ready.await(ms, TimeUnit.MILLISECONDS)

    override fun close() {
        send(byteArrayOf(Proto.BYE))
        h.postDelayed({
            closed = true
            stopAdvertising()
            try {
                central?.let { server?.cancelConnection(it) }
                server?.close()
                boost?.close()
            } catch (_: Exception) {
            }
            thread.quitSafely()
        }, 200)
    }
}
