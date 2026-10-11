package dev.flexdeck.app

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Typeface
import android.os.Build
import android.os.SystemClock
import android.view.View
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.sin

/**
 * The pairing picture, in characters: this tablet on the left, rings going out
 * of it, and the PC on the right lighting as each one lands. A native take on
 * `rings` in packages/ui/src/ascii/fields.ts, with the rings coloured by how far
 * out they are.
 *
 * Each frame is drawn as whole rows, one string per colour, not cell by cell:
 * a few dozen text draws a frame, cheap enough for the slowest tablets. It runs
 * at about 24 frames a second, and holds a single frame with animations off.
 */
class AsciiLinkView(context: Context) : View(context) {
    private val base = Paint(Paint.ANTI_ALIAS_FLAG).apply { typeface = Typeface.MONOSPACE; color = FAINT }
    private val glow = Paint(Paint.ANTI_ALIAS_FLAG).apply { typeface = Typeface.MONOSPACE }

    private val grid = Array(ROWS) { CharArray(COLS) }
    private val tone = Array(ROWS) { IntArray(COLS) }
    private val row = CharArray(COLS)

    private var cell = 0f
    private var line = 0f
    private var start = SystemClock.uptimeMillis()
    private var lastFrame = 0L
    private var still = false

    private val animator = ValueAnimator.ofFloat(0f, 1f).apply {
        duration = 1000
        repeatCount = ValueAnimator.INFINITE
        addUpdateListener {
            val now = SystemClock.uptimeMillis()
            if (now - lastFrame >= FRAME_MS) {
                lastFrame = now
                invalidate()
            }
        }
    }

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        // As large as fits both ways; a monospace cell is ~0.6 as wide as tall.
        base.textSize = 100f
        val ratio = base.measureText("0") / 100f
        val size = minOf(w / (COLS * ratio), h / (ROWS * LINE))
        base.textSize = size
        glow.textSize = size
        glow.setShadowLayer(size * 0.6f, 0f, 0f, Color.WHITE)
        cell = base.measureText("0")
        line = size * LINE
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        still = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !ValueAnimator.areAnimatorsEnabled()
        start = SystemClock.uptimeMillis()
        if (!still) animator.start()
    }

    override fun onDetachedFromWindow() {
        animator.cancel()
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        if (cell == 0f) return
        val t = if (still) 1.3f else (SystemClock.uptimeMillis() - start) / 1000f
        compose(t)

        val left = (width - cell * COLS) / 2f
        val top = (height - line * ROWS) / 2f - base.ascent()
        for (y in 0 until ROWS) {
            val baseline = top + y * line
            // The faint layer, then each colour over it.
            for (x in 0 until COLS) row[x] = if (tone[y][x] == 0) grid[y][x] else ' '
            canvas.drawText(row, 0, COLS, left, baseline, base)
            for (colour in GLOWS) {
                var any = false
                for (x in 0 until COLS) {
                    val mine = tone[y][x] == colour
                    row[x] = if (mine) grid[y][x] else ' '
                    any = any || mine
                }
                if (!any) continue
                glow.color = colour
                glow.setShadowLayer(glow.textSize * 0.6f, 0f, 0f, colour and 0x88FFFFFF.toInt())
                canvas.drawText(row, 0, COLS, left, baseline, glow)
            }
        }
    }

    /** Lay out one frame into [grid] and [tone]; tone 0 is the faint layer. */
    private fun compose(t: Float) {
        for (y in 0 until ROWS) {
            grid[y].fill(' ')
            tone[y].fill(0)
        }
        val cy = ROWS / 2

        // The rings, from the tablet's right edge, open toward the PC.
        val ex = TAB_X + TAB_W
        for (y in 0 until ROWS) for (x in ex + 1 until PC_X - 1) {
            val dx = (x - ex) * 0.55f
            val dy = (y - cy).toFloat()
            val r = hypot(dx, dy)
            if (abs(dy) > r * 0.92f || r > 17f) continue
            val wave = sin(r * 1.05f - t * 4.6f)
            if (wave > 0.9f) {
                val band = minOf(RING_CHARS.size - 1, (r / 3.6f).toInt())
                put(x, y, RING_CHARS[band], RING_TONES[band])
            } else if (wave > 0.55f && r < 11f) put(x, y, '.', 0)
        }

        // The tablet: a dotted bezel and a grid of keys, one lit as a ring leaves.
        val lit = floor(t * 4.6f / (2 * Math.PI.toFloat())).toInt()
        val litKey = Math.floorMod(lit * 7, KEYS_ACROSS * KEYS_DOWN)
        for (y in TAB_Y until TAB_Y + TAB_H) for (x in TAB_X until TAB_X + TAB_W) {
            val edgeX = x == TAB_X || x == TAB_X + TAB_W - 1
            val edgeY = y == TAB_Y || y == TAB_Y + TAB_H - 1
            when {
                edgeX && edgeY -> put(x, y, '+', 0)
                edgeY -> put(x, y, if (x % 2 == 0) '.' else ' ', 0)
                edgeX -> put(x, y, '|', if (x == TAB_X) RED else 0)
                else -> {
                    val lx = x - TAB_X - 2
                    val ly = y - TAB_Y - 2
                    if (lx < 0 || ly < 0) continue
                    val kx = lx / 3
                    val ky = ly / 3
                    if (kx >= KEYS_ACROSS || ky >= KEYS_DOWN || lx % 3 == 2 || ly % 3 == 2) continue
                    val on = ky * KEYS_ACROSS + kx == litKey && (t * 4.6f) % (2 * Math.PI.toFloat()) < 2.2f
                    put(x, y, if (on) '#' else ':', if (on) SIGNAL else 0)
                }
            }
        }

        // The PC: a screen on a stand, lit while a ring is arriving.
        val arrive = sin(((PC_X - ex) * 0.55f) * 1.05f - t * 4.6f) > 0.4f
        val top = cy - PC_H / 2
        for (y in top until top + PC_H) for (x in PC_X until PC_X + PC_W) {
            val edgeX = x == PC_X || x == PC_X + PC_W - 1
            val edgeY = y == top || y == top + PC_H - 1
            when {
                edgeY -> put(x, y, '=', if (arrive) BLUE else 0)
                edgeX -> put(x, y, '|', if (arrive) BLUE else 0)
                else -> put(x, y, ':', if (arrive) BLUE else 0)
            }
        }
        val mid = PC_X + PC_W / 2
        put(mid, top + PC_H, '|', 0)
        for (x in mid - 2..mid + 2) put(x, top + PC_H + 1, '-', 0)
    }

    private fun put(x: Int, y: Int, c: Char, colour: Int) {
        if (x !in 0 until COLS || y !in 0 until ROWS) return
        grid[y][x] = c
        tone[y][x] = colour
    }

    private companion object {
        const val COLS = 58
        const val ROWS = 21
        const val LINE = 1.18f
        const val FRAME_MS = 41L

        const val TAB_X = 1
        const val TAB_W = 14
        const val TAB_Y = 2
        const val TAB_H = 17
        const val KEYS_ACROSS = 4
        const val KEYS_DOWN = 4

        const val PC_X = 46
        const val PC_W = 11
        const val PC_H = 7

        val FAINT = Color.parseColor("#4A4A50")
        val SIGNAL = Color.parseColor("#FF5A1F")
        val RED = Color.parseColor("#F87171")
        val BLUE = Color.parseColor("#6CA0FF")
        val YELLOW = Color.parseColor("#F5C451")
        val GREEN = Color.parseColor("#4ADE80")

        val RING_CHARS = charArrayOf(')', '}', ')', ']', ']')
        val RING_TONES = intArrayOf(GREEN, BLUE, YELLOW, SIGNAL, RED)
        val GLOWS = intArrayOf(SIGNAL, RED, BLUE, YELLOW, GREEN)
    }
}
