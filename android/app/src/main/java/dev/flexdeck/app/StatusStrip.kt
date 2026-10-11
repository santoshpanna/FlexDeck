package dev.flexdeck.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Typeface
import android.os.SystemClock
import android.view.View
import kotlin.math.sin

/**
 * The thin readout across the top of the deck: a light and a line of mono
 * text — `ONLINE · REV 042`, `RECONNECTING IN 4S` — in place of the grey status
 * line that used to be the only sign anything was wrong.
 *
 * It asks for frames only while its light is breathing (waiting), so a panel
 * sitting online costs nothing.
 */
class StatusStrip(context: Context) : View(context) {

    enum class Light { ONLINE, WAITING, FAULT }

    private var light = Light.WAITING
    private var text = "CONNECTING"
    private var detail = ""

    private val density = resources.displayMetrics.density
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.MONOSPACE
        letterSpacing = 0.18f
        textSize = 11f * density
        color = INK
    }
    private val dimPaint = Paint(textPaint).apply { color = MUTED }
    private val ledPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val glowPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val linePaint = Paint().apply { color = LINE }

    /** Set what the strip says. `detail` sits dimmed on the right: the agent's address. */
    fun show(light: Light, text: String, detail: String = this.detail) {
        this.light = light
        this.text = text.uppercase()
        this.detail = detail.uppercase()
        invalidate()
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        setMeasuredDimension(MeasureSpec.getSize(widthMeasureSpec), (36 * density).toInt())
    }

    override fun onDraw(canvas: Canvas) {
        canvas.drawColor(BACKGROUND)
        canvas.drawRect(0f, height - density, width.toFloat(), height.toFloat(), linePaint)

        val pad = 20f * density
        val cy = height / 2f
        val color = when (light) {
            Light.ONLINE -> OK
            Light.WAITING -> WAIT
            Light.FAULT -> BAD
        }
        val alpha = if (light == Light.WAITING) {
            (0.3 + 0.7 * (sin(SystemClock.uptimeMillis() / 320.0) + 1) / 2).toFloat()
        } else {
            1f
        }
        glowPaint.color = color
        glowPaint.alpha = (60 * alpha).toInt()
        canvas.drawCircle(pad, cy, 7f * density, glowPaint)
        ledPaint.color = color
        ledPaint.alpha = (255 * alpha).toInt()
        canvas.drawCircle(pad, cy, 3.5f * density, ledPaint)

        val baseline = cy + textPaint.textSize * 0.35f
        canvas.drawText(text, pad + 14f * density, baseline, textPaint)
        if (detail.isNotEmpty()) {
            canvas.drawText(detail, width - pad - dimPaint.measureText(detail), baseline, dimPaint)
        }

        if (light == Light.WAITING) postInvalidateOnAnimation()
    }

    private companion object {
        val BACKGROUND = Color.parseColor("#0A0A0B")
        val LINE = Color.parseColor("#17171A")
        val INK = Color.parseColor("#ECEBE6")
        val MUTED = Color.parseColor("#5F5E5A")
        val OK = Color.parseColor("#4ADE80")
        val WAIT = Color.parseColor("#FBBF24")
        val BAD = Color.parseColor("#F87171")
    }
}
