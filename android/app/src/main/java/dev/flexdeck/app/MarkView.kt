package dev.flexdeck.app

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.os.Build
import android.view.View
import android.view.animation.LinearInterpolator
import kotlin.math.pow

/**
 * The FlexDeck mark, `[f]`, pressing itself on a loop: the brackets close in on
 * the f and it lights, like a keypress. Rests pressed and lit when the system
 * has animations turned off.
 *
 * The shapes are copied from packages/ui/src/brand/mark.ts (a 64-unit square),
 * the source the web `Mark` and every icon file are drawn from. Keep them in step.
 */
class MarkView(context: Context) : View(context) {
    private val bracketPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = FAINT }
    private val letterPaint = Paint(Paint.ANTI_ALIAS_FLAG)

    private val left = Path().apply {
        rect(15f, 12f, S, 40f); rect(15f, 12f, 8f, S); rect(15f, 52f - S, 8f, S)
    }
    private val right = Path().apply {
        rect(49f - S, 12f, S, 40f); rect(41f, 12f, 8f, S); rect(41f, 52f - S, 8f, S)
    }
    private val letter = Path().apply {
        rect(28.5f, 22f, S, 30f)
        // The hook: the top-left quarter of a ring centred on (36.5, 22).
        val cx = 36.5f
        val cy = 22f
        val r = 8f
        val inner = r - S
        moveTo(cx - r, cy)
        arcTo(RectF(cx - r, cy - r, cx + r, cy + r), 180f, 90f)
        lineTo(cx, cy - inner)
        arcTo(RectF(cx - inner, cy - inner, cx + inner, cy + inner), 270f, -90f)
        close()
        rect(36.5f, 14f, 2.5f, S)
        rect(25f, 26f, 14f, S)
    }

    /** 0 = released (brackets open, f ink), 1 = pressed (shut, f lit). */
    private var press = 1f

    private val animator = ValueAnimator.ofFloat(0f, 1f).apply {
        duration = CYCLE_MS
        repeatCount = ValueAnimator.INFINITE
        interpolator = LinearInterpolator()
        addUpdateListener {
            press = pressAt(it.animatedValue as Float)
            invalidate()
        }
    }

    private fun Path.rect(x: Float, y: Float, w: Float, h: Float) = addRect(x, y, x + w, y + h, Path.Direction.CW)

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val h = MeasureSpec.getSize(heightMeasureSpec)
        setMeasuredDimension((h * BOX_W / BOX_H).toInt(), h)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        val enabled = Build.VERSION.SDK_INT < Build.VERSION_CODES.O || ValueAnimator.areAnimatorsEnabled()
        if (enabled) animator.start()
    }

    override fun onDetachedFromWindow() {
        animator.cancel()
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        val k = height / BOX_H
        canvas.save()
        canvas.scale(k, k)
        canvas.translate(-BOX_X, -BOX_Y)
        val open = (1f - press) * TRAVEL
        canvas.save(); canvas.translate(-open, 0f); canvas.drawPath(left, bracketPaint); canvas.restore()
        canvas.save(); canvas.translate(open, 0f); canvas.drawPath(right, bracketPaint); canvas.restore()
        letterPaint.color = blend(INK, SIGNAL, press)
        canvas.drawPath(letter, letterPaint)
        canvas.restore()
    }

    private companion object {
        const val S = 4.5f
        const val TRAVEL = 4f
        const val CYCLE_MS = 2400L
        // The bare glyph's box, with room for the brackets to open (as in Mark.component.tsx).
        const val BOX_X = 9f
        const val BOX_Y = 10f
        const val BOX_W = 46f
        const val BOX_H = 44f

        val INK = Color.parseColor("#ECEBE6")
        val FAINT = Color.parseColor("#5F5E5A")
        val SIGNAL = Color.parseColor("#FF5A1F")

        /** Open for a beat, snap shut, hold, let go: the web mark's keyframes. */
        fun pressAt(t: Float): Float = when {
            t < 0.55f -> 0f
            t < 0.60f -> outExpo((t - 0.55f) / 0.05f)
            t < 0.94f -> 1f
            else -> 1f - outExpo((t - 0.94f) / 0.06f)
        }

        fun outExpo(t: Float) = if (t >= 1f) 1f else 1f - 2f.pow(-10f * t)

        fun blend(a: Int, b: Int, t: Float): Int = Color.rgb(
            (Color.red(a) + (Color.red(b) - Color.red(a)) * t).toInt(),
            (Color.green(a) + (Color.green(b) - Color.green(a)) * t).toInt(),
            (Color.blue(a) + (Color.blue(b) - Color.blue(a)) * t).toInt(),
        )
    }
}
