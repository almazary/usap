package com.almazari.usap

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.os.Build
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View

/** Layar trackpad: membaca sentuhan multi-jari dan mengirimnya ke agen dalam milimeter. */
@SuppressLint("ViewConstructor")
class PadView(ctx: Context, private val link: Conn) : View(ctx) {
    private val mmX: Float
    private val mmY: Float
    private val ids = IntArray(10)
    private val xs = FloatArray(10)
    private val ys = FloatArray(10)
    private val dots = HashMap<Int, Pair<Float, Float>>()
    private var cancels = 0
    private val d = resources.displayMetrics.density

    private val surface = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.rgb(0x1B, 0x1F, 0x24) }
    private val edge = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(0x2C, 0x33, 0x3B); style = Paint.Style.STROKE; strokeWidth = 1.5f * d
    }
    private val dot = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.argb(70, 0x8F, 0xB3, 0xD9) }
    private val title = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(0x8A, 0x95, 0xA3); textSize = 15 * d; textAlign = Paint.Align.CENTER
    }
    private val hint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(0x5A, 0x64, 0x70); textSize = 12 * d; textAlign = Paint.Align.CENTER
    }
    private val status = Paint(Paint.ANTI_ALIAS_FLAG)
    private val battText = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(0x8A, 0x95, 0xA3); textSize = 13 * d; textAlign = Paint.Align.RIGHT
    }
    private val battLine = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE; strokeWidth = 1.5f * d }
    private val battFill = Paint(Paint.ANTI_ALIAS_FLAG)
    private var battPct = -1
    private var battCharging = false

    /** Baca status baterai yang sudah dipegang Android (tanpa mendaftar apa pun, jadi tidak menambah beban). */
    private fun readBattery() {
        val i: Intent = context.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED)) ?: return
        val level = i.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = i.getIntExtra(BatteryManager.EXTRA_SCALE, 100)
        if (level >= 0 && scale > 0) battPct = level * 100 / scale
        val st = i.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
        battCharging = st == BatteryManager.BATTERY_STATUS_CHARGING || st == BatteryManager.BATTERY_STATUS_FULL
    }

    private val heartbeat = object : Runnable {
        override fun run() {
            link.ping()
            readBattery()
            invalidate()
            postDelayed(this, 1000)
        }
    }

    init {
        val dm = resources.displayMetrics
        // beberapa HP melaporkan xdpi yang keliru; kalau tidak masuk akal pakai densityDpi
        val xdpi = if (dm.xdpi in 100f..1000f) dm.xdpi else dm.densityDpi.toFloat()
        val ydpi = if (dm.ydpi in 100f..1000f) dm.ydpi else dm.densityDpi.toFloat()
        mmX = 25.4f / xdpi
        mmY = 25.4f / ydpi
        setBackgroundColor(Color.rgb(0x12, 0x14, 0x17))
        isHapticFeedbackEnabled = false
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        post(heartbeat)
    }

    override fun onDetachedFromWindow() {
        removeCallbacks(heartbeat)
        super.onDetachedFromWindow()
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        super.onLayout(changed, l, t, r, b)
        // Minta Android tidak memakai usapan dari tepi kiri/kanan sebagai gestur kembali.
        // Android hanya mengabulkan paling tinggi 200dp per tepi, jadi pilih bagian tengah.
        if (Build.VERSION.SDK_INT >= 29) {
            val w = r - l
            val h = b - t
            val strip = (40 * d).toInt()
            val half = minOf(h / 2, (100 * d).toInt())
            val top = h / 2 - half
            val bottom = h / 2 + half
            systemGestureExclusionRects = listOf(Rect(0, top, strip, bottom), Rect(w - strip, top, w, bottom))
        }
    }

    private fun snapshot(e: MotionEvent, h: Int, skip: Int) {
        var n = 0
        for (i in 0 until e.pointerCount) {
            if (i == skip || n >= 10) continue
            ids[n] = e.getPointerId(i) and 0xff
            xs[n] = (if (h < 0) e.getX(i) else e.getHistoricalX(i, h)) * mmX
            ys[n] = (if (h < 0) e.getY(i) else e.getHistoricalY(i, h)) * mmY
            n++
        }
        val t = if (h < 0) e.eventTime else e.getHistoricalEventTime(h)
        link.touch(t, n, ids, xs, ys)
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                // kirim setiap gerakan begitu dibaca layar, tanpa menunggu frame berikutnya
                requestUnbufferedDispatch(e)
                snapshot(e, -1, -1)
            }
            MotionEvent.ACTION_MOVE -> {
                for (h in 0 until e.historySize) snapshot(e, h, -1)
                snapshot(e, -1, -1)
            }
            MotionEvent.ACTION_POINTER_DOWN -> snapshot(e, -1, -1)
            MotionEvent.ACTION_POINTER_UP -> snapshot(e, -1, e.actionIndex)
            MotionEvent.ACTION_UP -> link.touch(e.eventTime, 0, ids, xs, ys)
            MotionEvent.ACTION_CANCEL -> {
                cancels++
                link.cancelled()
                link.touch(e.eventTime, 0, ids, xs, ys)
            }
        }
        dots.clear()
        val ended = e.actionMasked == MotionEvent.ACTION_UP || e.actionMasked == MotionEvent.ACTION_CANCEL
        if (!ended) {
            for (i in 0 until e.pointerCount) {
                if (e.actionMasked == MotionEvent.ACTION_POINTER_UP && i == e.actionIndex) continue
                dots[e.getPointerId(i)] = e.getX(i) to e.getY(i)
            }
        }
        invalidate()
        return true
    }

    override fun onDraw(c: Canvas) {
        val m = 12 * d
        val r = RectF(m, m, width - m, height - m)
        c.drawRoundRect(r, 18 * d, 18 * d, surface)
        c.drawRoundRect(r, 18 * d, 18 * d, edge)

        val alive = SystemClock.uptimeMillis() - link.lastPong < 3500
        status.color = if (alive) Color.rgb(0x5F, 0xC4, 0x8A) else Color.rgb(0xE0, 0x9A, 0x4A)
        val name = link.computerName.ifEmpty { "komputer" }
        val label = if (alive) "Tersambung ke $name${link.label}" else "Menunggu $name..."
        val ty = m + 34 * d
        val tw = title.measureText(label)
        c.drawCircle(width / 2f - tw / 2 - 10 * d, ty - 5 * d, 4 * d, status)
        c.drawText(label, width / 2f, ty, title)

        if (battPct >= 0) {
            // ikon baterai kecil di pojok kanan atas, merah kalau tinggal sedikit dan tidak dicas
            val low = battPct <= 15 && !battCharging
            val tint = when {
                low -> Color.rgb(0xE0, 0x6C, 0x5A)
                battCharging -> Color.rgb(0x5F, 0xC4, 0x8A)
                else -> Color.rgb(0x8A, 0x95, 0xA3)
            }
            battLine.color = tint
            battFill.color = tint
            battText.color = tint
            val right = width - m - 22 * d
            val bw = 22 * d
            val bh = 11 * d
            val body = RectF(right - bw, ty - 5 * d - bh / 2, right, ty - 5 * d + bh / 2)
            c.drawRoundRect(body, 2.5f * d, 2.5f * d, battLine)
            c.drawRect(right + 1 * d, body.centerY() - 2.5f * d, right + 3 * d, body.centerY() + 2.5f * d, battFill)
            val inner = RectF(body.left + 2 * d, body.top + 2 * d, body.left + 2 * d + (bw - 4 * d) * battPct / 100f, body.bottom - 2 * d)
            c.drawRect(inner, battFill)
            val txt = if (battCharging) "$battPct% mengisi" else "$battPct%"
            c.drawText(txt, body.left - 6 * d, ty, battText)
        }

        val lines = listOf(
            "1 jari: gerak  ·  ketuk: klik  ·  tahan telunjuk, geser jari tengah: blok/seret",
            "2 jari: scroll  ·  ketuk 2 jari: klik kanan",
            "3 jari ke atas: Mission Control  ·  ke kiri/kanan: ganti desktop",
            if (cancels > 0) "Sentuhan dipotong sistem $cancels kali. Hindari usapan dari tepi layar." else "Tombol kembali: putuskan",
        )
        var y = height - m - 20 * d - (lines.size - 1) * 18 * d
        for (l in lines) {
            c.drawText(l, width / 2f, y, hint)
            y += 18 * d
        }
        for ((x, yy) in dots.values) c.drawCircle(x, yy, 26 * d, dot)
    }
}
