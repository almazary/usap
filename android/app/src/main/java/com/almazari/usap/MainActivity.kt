package com.almazari.usap

import android.app.Activity
import android.Manifest
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.content.Context
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbAccessory
import android.hardware.usb.UsbManager
import android.content.pm.PackageManager
import android.content.pm.ActivityInfo
import android.graphics.Color
import android.graphics.Typeface
import android.net.ConnectivityManager
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.text.InputFilter
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.SocketTimeoutException

class MainActivity : Activity() {
    private val ui = Handler(Looper.getMainLooper())
    private var link: Conn? = null
    private var pad: PadView? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private lateinit var hostInput: EditText
    private lateinit var codeInput: EditText
    private lateinit var statusText: TextView
    private lateinit var foundList: LinearLayout
    private val d get() = resources.displayMetrics.density

    private val fg = Color.rgb(0xE8, 0xEE, 0xF5)
    private val muted = Color.rgb(0x8A, 0x95, 0xA3)
    private val accent = Color.rgb(0x8F, 0xB3, 0xD9)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.decorView.setBackgroundColor(Color.rgb(0x12, 0x14, 0x17))
        showSetup()
        handleAccessoryIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleAccessoryIntent(intent)
    }

    /** Usap dibuka Android karena HP baru dicolok ke usap-agent (mode aksesori). */
    private fun handleAccessoryIntent(i: Intent?) {
        if (i?.action != UsbManager.ACTION_USB_ACCESSORY_ATTACHED) return
        @Suppress("DEPRECATION")
        val acc: UsbAccessory? = i.getParcelableExtra(UsbManager.EXTRA_ACCESSORY)
        if (acc != null) connectAoa(acc)
    }

    private val usbPermission = "com.almazari.usap.USB_PERMISSION"
    private var usbReceiver: BroadcastReceiver? = null

    private fun connectAoa(given: UsbAccessory?) {
        val um = getSystemService(UsbManager::class.java)
        val acc = given ?: um.accessoryList?.firstOrNull { it.manufacturer == "Usap" }
        if (acc == null) {
            statusText.text = "Belum ada kabel ke usap-agent. Colok HP ke komputer yang menjalankan usap-agent; Usap akan terbuka sendiri."
            return
        }
        if (!um.hasPermission(acc)) {
            if (usbReceiver == null) {
                usbReceiver = object : BroadcastReceiver() {
                    override fun onReceive(c: Context, it: Intent) {
                        if (it.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) connectAoa(acc)
                        else statusText.text = "Izin kabel ditolak. Tekan Sambungkan lewat kabel untuk mencoba lagi."
                    }
                }
                val f = IntentFilter(usbPermission)
                if (Build.VERSION.SDK_INT >= 33) registerReceiver(usbReceiver, f, Context.RECEIVER_NOT_EXPORTED)
                else registerReceiver(usbReceiver, f)
            }
            val pi = PendingIntent.getBroadcast(this, 0, Intent(usbPermission).setPackage(packageName), PendingIntent.FLAG_MUTABLE)
            um.requestPermission(acc, pi)
            return
        }
        if (link is AoaLink) return
        link?.close()
        link = null
        val code = prefs().getString("code", "") ?: ""
        statusText.text = "Menyambungkan lewat kabel..."
        Thread {
            val l = try {
                AoaLink(this, acc, code.padEnd(6, '0').take(6), "${Build.MANUFACTURER} ${Build.MODEL}")
            } catch (e: Exception) {
                ui.post { statusText.text = "Kabel tidak bisa dibuka. Cabut lalu colok lagi." }
                return@Thread
            }
            finishHello(l, "usap-agent tidak menjawab lewat kabel. Cabut lalu colok lagi.")
        }.start()
    }

    private fun prefs() = getSharedPreferences("usap", Context.MODE_PRIVATE)

    private fun text(s: String, size: Float, color: Int, bold: Boolean = false) = TextView(this).apply {
        text = s
        textSize = size
        setTextColor(color)
        if (bold) typeface = Typeface.DEFAULT_BOLD
    }

    private fun input(hintText: String, value: String) = EditText(this).apply {
        hint = hintText
        setText(value)
        setTextColor(fg)
        setHintTextColor(Color.rgb(0x5A, 0x64, 0x70))
        textSize = 17f
        isSingleLine = true
    }

    /** Minta Wi-Fi tidak hemat daya selama trackpad dipakai, supaya paket tidak dikirim bergerombol. */
    private fun holdWifi(on: Boolean) {
        if (on) {
            if (wifiLock == null) {
                val wm = applicationContext.getSystemService(WifiManager::class.java)
                val mode = if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY
                else @Suppress("DEPRECATION") WifiManager.WIFI_MODE_FULL_HIGH_PERF
                wifiLock = wm.createWifiLock(mode, "usap").apply { setReferenceCounted(false) }
            }
            wifiLock?.acquire()
        } else {
            wifiLock?.let { if (it.isHeld) it.release() }
        }
    }

    private fun showSetup() {
        pad = null
        holdWifi(false)
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        fullScreen(false)
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val p = prefs()
        val col = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            val g = (24 * d).toInt()
            setPadding(g, (48 * d).toInt(), g, g)
        }
        col.addView(text("Usap", 32f, fg, bold = true))
        col.addView(text("Jadikan HP ini trackpad untuk komputer. Jalankan usap-agent di komputer, lalu sambungkan HP ke Wi-Fi yang sama.", 15f, muted).apply {
            setPadding(0, (8 * d).toInt(), 0, (24 * d).toInt())
        })

        col.addView(Button(this).apply {
            text = "Cari komputer"
            setOnClickListener { discover() }
        })
        foundList = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        col.addView(foundList)

        col.addView(text("Alamat IP komputer", 13f, muted).apply { setPadding(0, (20 * d).toInt(), 0, 0) })
        hostInput = input("misalnya 192.168.100.2", p.getString("host", "") ?: "").apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
        }
        col.addView(hostInput)
        col.addView(text("Kode 6 angka dari agen", 13f, muted).apply { setPadding(0, (12 * d).toInt(), 0, 0) })
        codeInput = input("tampil di terminal saat agen berjalan", p.getString("code", "") ?: "").apply {
            inputType = InputType.TYPE_CLASS_NUMBER
            filters = arrayOf(InputFilter.LengthFilter(6))
        }
        col.addView(codeInput)

        col.addView(Button(this).apply {
            text = "Sambungkan"
            setOnClickListener { connect() }
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = (20 * d).toInt() })
        col.addView(Button(this).apply {
            text = "Sambungkan lewat kabel"
            setOnClickListener { connectAoa(null) }
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = (8 * d).toInt() })
        col.addView(text("Paling akurat. Colok HP ke komputer yang menjalankan usap-agent; Usap terbuka sendiri. Tidak perlu mode developer.", 13f, muted))
        col.addView(Button(this).apply {
            text = "Sambungkan lewat Bluetooth"
            setOnClickListener { connectBle() }
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = (8 * d).toInt() })
        col.addView(text("Tanpa kabel dan tidak bergantung pada Wi-Fi. usap-agent di komputer akan menyambung sendiri ke HP ini.", 13f, muted))
        col.addView(Button(this).apply {
            text = "Kabel lewat USB debugging"
            isAllCaps = false
            setOnClickListener { connect(usb = true) }
        }, LinearLayout.LayoutParams(-1, -2).apply { topMargin = (8 * d).toInt() })
        col.addView(text("Cadangan untuk HP yang mode developer-nya sudah menyala.", 13f, muted))
        statusText = text("", 14f, accent).apply { setPadding(0, (12 * d).toInt(), 0, 0) }
        col.addView(statusText)

        setContentView(ScrollView(this).apply { addView(col) })
        if (hostInput.text.isEmpty()) discover()
    }

    private fun broadcastAddresses(): List<InetAddress> {
        val out = mutableListOf<InetAddress>(InetAddress.getByName("255.255.255.255"))
        try {
            val cm = getSystemService(ConnectivityManager::class.java)
            val lp = cm.getLinkProperties(cm.activeNetwork)
            lp?.linkAddresses?.forEach { la ->
                val a = la.address
                if (a is Inet4Address && la.prefixLength in 8..30) {
                    val ip = a.address.fold(0) { acc, b -> (acc shl 8) or (b.toInt() and 0xff) }
                    val bc = ip or ((1 shl (32 - la.prefixLength)) - 1)
                    out.add(InetAddress.getByAddress(byteArrayOf((bc ushr 24).toByte(), (bc ushr 16).toByte(), (bc ushr 8).toByte(), bc.toByte())))
                }
            }
        } catch (_: Exception) {
        }
        return out
    }

    private fun discover() {
        statusText.text = "Mencari komputer di jaringan ini..."
        foundList.removeAllViews()
        Thread {
            val found = LinkedHashMap<String, String>()
            try {
                DatagramSocket().use { s ->
                    s.broadcast = true
                    s.soTimeout = 250
                    val msg = byteArrayOf(Proto.DISCOVER)
                    val targets = broadcastAddresses()
                    val until = System.currentTimeMillis() + 1500
                    var nextSend = 0L
                    val buf = ByteArray(256)
                    while (System.currentTimeMillis() < until) {
                        if (System.currentTimeMillis() >= nextSend) {
                            targets.forEach { t -> try { s.send(DatagramPacket(msg, 1, t, Link.PORT)) } catch (_: Exception) {} }
                            nextSend = System.currentTimeMillis() + 500
                        }
                        try {
                            val p = DatagramPacket(buf, buf.size)
                            s.receive(p)
                            if (p.length > 0 && (buf[0].toInt() and 0xff) == Proto.HERE) {
                                val ip = p.address.hostAddress; if (ip != null) found[ip] = String(buf, 1, p.length - 1, Charsets.UTF_8)
                            }
                        } catch (_: SocketTimeoutException) {
                        }
                    }
                }
            } catch (_: Exception) {
            }
            ui.post {
                if (pad != null) return@post
                foundList.removeAllViews()
                if (found.isEmpty()) {
                    statusText.text = "Tidak menemukan komputer. Pastikan usap-agent berjalan, atau ketik alamat IP-nya."
                } else {
                    statusText.text = "Pilih komputer:"
                    found.forEach { (ip, name) ->
                        foundList.addView(Button(this).apply {
                            text = "$name  ($ip)"
                            isAllCaps = false
                            setOnClickListener {
                                hostInput.setText(ip)
                                codeInput.requestFocus()
                                statusText.text = "Masukkan kode dari terminal, lalu Sambungkan."
                            }
                        })
                    }
                }
            }
        }.start()
    }

    private fun connect(usb: Boolean = false) {
        val host = if (usb) "127.0.0.1" else hostInput.text.toString().trim()
        val code = codeInput.text.toString().trim()
        if (host.isEmpty()) { statusText.text = "Isi alamat IP komputer dulu, atau tekan Cari komputer."; return }
        if (code.length != 6) { statusText.text = "Kode harus 6 angka, sesuai yang tampil di terminal."; return }
        if (!usb) prefs().edit().putString("host", host).apply()
        prefs().edit().putString("code", code).apply()
        statusText.text = "Menyambungkan..."
        Thread {
            val l = try {
                Link(InetAddress.getByName(host), Link.PORT, code, "${Build.MANUFACTURER} ${Build.MODEL}", usb)
            } catch (e: Exception) {
                ui.post {
                    statusText.text = if (usb) "Tidak bisa menyambung lewat USB. Pastikan kabel tercolok, USB debugging menyala dan diizinkan, dan usap-agent berjalan."
                    else "Alamat tidak valid: $host"
                }
                return@Thread
            }
            finishHello(l, if (usb) "Komputer tidak menjawab lewat USB. Jalankan ulang usap-agent lalu coba lagi." else "Komputer tidak menjawab. Pastikan usap-agent berjalan dan HP di Wi-Fi yang sama.")
        }.start()
    }

    /** Kirim kode ke agen lalu buka trackpad kalau diterima. Dipanggil dari thread latar. */
    private fun finishHello(l: Conn, noAnswer: String) {
        val r = l.hello()
        ui.post {
            when (r) {
                1 -> { link = l; showPad(l) }
                0 -> { l.close(); statusText.text = "Kode salah. Cek kode di terminal komputer." }
                2 -> { l.close(); statusText.text = "Terlalu banyak kode salah. Tunggu 1 menit lalu coba lagi." }
                else -> { l.close(); statusText.text = noAnswer }
            }
        }
    }

    private val bleRequest = 7
    private val bleEnable = 8
    private var bleConnecting: BleLink? = null

    private fun connectBle() {
        val code = codeInput.text.toString().trim()
        if (code.length != 6) { statusText.text = "Kode harus 6 angka, sesuai yang tampil di terminal."; return }
        if (Build.VERSION.SDK_INT >= 31) {
            val need = arrayOf(Manifest.permission.BLUETOOTH_ADVERTISE, Manifest.permission.BLUETOOTH_CONNECT)
            if (need.any { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }) {
                requestPermissions(need, bleRequest)
                return
            }
        }
        val adapter = getSystemService(BluetoothManager::class.java)?.adapter
        if (adapter == null) { statusText.text = "HP ini tidak punya Bluetooth."; return }
        if (!adapter.isEnabled) {
            @Suppress("DEPRECATION")
            startActivityForResult(Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE), bleEnable)
            return
        }
        prefs().edit().putString("code", code).apply()
        bleConnecting?.close()
        val l = BleLink(this, code, "${Build.MANUFACTURER} ${Build.MODEL}")
        bleConnecting = l
        val err = l.start()
        if (err != null) { l.close(); bleConnecting = null; statusText.text = err; return }
        statusText.text = "Menunggu komputer menyambung lewat Bluetooth... Pastikan usap-agent berjalan dan Bluetooth Mac menyala."
        Thread {
            if (!l.awaitReady(30_000)) {
                ui.post {
                    if (bleConnecting === l) {
                        l.close(); bleConnecting = null
                        statusText.text = "Komputer tidak menyambung dalam 30 detik. Pastikan usap-agent berjalan, lalu coba lagi."
                    }
                }
                return@Thread
            }
            ui.post { statusText.text = "Tersambung lewat Bluetooth, mengirim kode..." }
            finishHello(l, "Komputer tidak menjawab lewat Bluetooth. Coba lagi.")
            ui.post { if (bleConnecting === l) bleConnecting = null }
        }.start()
    }

    @Deprecated("Deprecated in Java")
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        if (requestCode == bleRequest) {
            if (grantResults.isNotEmpty() && grantResults.all { it == PackageManager.PERMISSION_GRANTED }) connectBle()
            else statusText.text = "Usap butuh izin Perangkat di sekitar untuk memakai Bluetooth."
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == bleEnable) {
            if (resultCode == RESULT_OK) connectBle() else statusText.text = "Bluetooth perlu dinyalakan untuk sambungan Bluetooth."
        }
    }

    private fun vibrator(): Vibrator =
        if (Build.VERSION.SDK_INT >= 31) getSystemService(VibratorManager::class.java).defaultVibrator
        else @Suppress("DEPRECATION") getSystemService(Vibrator::class.java)

    private fun showPad(l: Conn) {
        val v = vibrator()
        val click = if (Build.VERSION.SDK_INT >= 29) VibrationEffect.createPredefined(VibrationEffect.EFFECT_CLICK)
        else VibrationEffect.createOneShot(12, VibrationEffect.DEFAULT_AMPLITUDE)
        l.onHaptic = { v.vibrate(click) }
        l.onLost = {
            ui.post {
                if (link === l) {
                    l.close()
                    link = null
                    showSetup()
                    statusText.text = "Kabel dilepas."
                }
            }
        }
        val view = PadView(this, l)
        pad = view
        setContentView(view)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        fullScreen(true)
        if (l.usesWifi) holdWifi(true)
        // trackpad dipakai rebah supaya ruang geser kiri-kanan lebih lebar; boleh diputar ke dua sisi
        requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
    }

    private fun fullScreen(on: Boolean) {
        if (Build.VERSION.SDK_INT >= 30) {
            val c = window.insetsController ?: return
            if (on) {
                c.hide(WindowInsets.Type.systemBars())
                c.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            } else c.show(WindowInsets.Type.systemBars())
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility = if (on)
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
            else View.SYSTEM_UI_FLAG_VISIBLE
        }
    }

    override fun onResume() {
        super.onResume()
        val l = link ?: return
        // agen mungkin baru dijalankan ulang: sapa lagi supaya tetap diterima
        if (pad != null) Thread { l.hello() }.start()
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (pad != null) {
            link?.close()
            link = null
            showSetup()
        } else {
            @Suppress("DEPRECATION")
            super.onBackPressed()
        }
    }

    override fun onDestroy() {
        holdWifi(false)
        bleConnecting?.close()
        usbReceiver?.let { unregisterReceiver(it) }
        link?.close()
        link = null
        super.onDestroy()
    }
}
