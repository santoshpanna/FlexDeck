package dev.flexdeck.app

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RadialGradient
import android.graphics.RectF
import android.graphics.Shader
import android.graphics.Typeface
import dev.flexdeck.client.BackgroundSpec
import dev.flexdeck.client.Box
import dev.flexdeck.client.Clock
import dev.flexdeck.client.Colors
import dev.flexdeck.client.ElementLive
import dev.flexdeck.client.ElementSpec
import dev.flexdeck.client.Fade
import dev.flexdeck.client.FaceLayout
import dev.flexdeck.client.FaceSpec
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

/**
 * Draws a key's face: its background, then each element in its place.
 *
 * Where each place is comes from [FaceLayout] (the same numbers as the
 * editor's preview); what each element says comes from the agent, except a
 * clock, which reads the tablet's own time. Text is wrapped and sized once per
 * change and kept, so a frame spent springing a key allocates nothing new.
 */
internal class FacePainter(
    /** Draw an icon `size` square at (`left`, `top`); whether there was one. */
    private val icon: (Canvas, String?, Float, Float, Float, Int) -> Boolean,
    /** A picture by sha256, or `null` while it is on its way (it is asked for). */
    private val picture: (String) -> Bitmap?,
) {
    var unit = 1f

    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }
    private val text = Paint(Paint.ANTI_ALIAS_FLAG)
    private val bitmapPaint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private val clip = Path()
    private val graphPath = Path()
    private val area = RectF()
    private val src = android.graphics.Rect()
    private val slot = RectF()

    /** Text already wrapped and sized, by key: what to draw while nothing changes. */
    private class Laid(val source: String, val box: Box, val px: Float, val typeface: Typeface, val maxLines: Int, val lines: List<String>)

    private val laid = HashMap<String, Laid>()

    /** Forget wrapped text: the deck changed. */
    fun reset() = laid.clear()

    fun parse(color: String?): Int? = Colors.parse(color)

    // ---- Backgrounds -------------------------------------------------------

    /** A face's fade from `from` to `to`: a line at its angle, or a circle from the middle. */
    private fun fade(frame: RectF, spec: BackgroundSpec, from: Int, to: Int): Shader {
        if (spec.radial) {
            return RadialGradient(frame.centerX(), frame.centerY(), Fade.radius(frame.width(), frame.height()), from, to, Shader.TileMode.CLAMP)
        }
        val (x0, y0, x1, y1) = Fade.line(frame.width(), frame.height(), spec.angle)
        return LinearGradient(frame.left + x0, frame.top + y0, frame.left + x1, frame.top + y1, from, to, Shader.TileMode.CLAMP)
    }

    /**
     * Fill `frame`: the face's colour or fade, its picture, its graph. Returns
     * whether anything was drawn (a transparent face draws nothing).
     */
    fun background(
        canvas: Canvas,
        frame: RectF,
        radius: Float,
        layout: String,
        spec: BackgroundSpec,
        ruleColor: String,
        image: String,
        series: List<Double>?,
        machined: Shader,
    ): Boolean {
        if (spec.transparent && ruleColor.isEmpty()) return false
        val color = parse(ruleColor) ?: parse(spec.color)
        val to = parse(spec.colorTo)
        fill.shader = when {
            color != null && to != null -> fade(frame, spec, color, to)
            color != null -> null
            else -> machined
        }
        fill.color = color ?: Color.BLACK
        canvas.drawRoundRect(frame, radius, radius, fill)
        fill.shader = null

        val sha = image.removePrefix("img:").takeIf { image.startsWith("img:") }
        val bitmap = sha?.let(picture)
        if (bitmap != null) {
            canvas.save()
            clip.reset()
            clip.addRoundRect(frame, radius, radius, Path.Direction.CW)
            canvas.clipPath(clip)
            drawBitmap(canvas, bitmap, frame, spec.contain)
            val dim = if (spec.dim > 0) spec.dim else 0
            if (dim > 0) {
                fill.color = Color.argb(dim * 255 / 100, 0, 0, 0)
                canvas.drawRect(frame, fill)
            }
            // A picture under a title: a shade up from the bottom so it reads.
            if (layout == "FACE_LAYOUT_ART") {
                fill.shader = LinearGradient(
                    0f, frame.top + frame.height() * 0.4f, 0f, frame.bottom,
                    Color.TRANSPARENT, Color.argb(200, 0, 0, 0), Shader.TileMode.CLAMP,
                )
                canvas.drawRect(frame, fill)
                fill.shader = null
            }
            canvas.restore()
        }

        if (series != null && series.size >= 2) {
            canvas.save()
            clip.reset()
            clip.addRoundRect(frame, radius, radius, Path.Direction.CW)
            canvas.clipPath(clip)
            val accent = parse(spec.graphColor) ?: SIGNAL
            area.set(frame.left, frame.top + frame.height() * 0.35f, frame.right, frame.bottom)
            graph(canvas, area, series, 0.0, 0.0, "GRAPH_STYLE_AREA", 0, accent, faint = true)
            canvas.restore()
        }
        return true
    }

    /** A picture covering or fitting inside `into`. */
    fun drawBitmap(canvas: Canvas, bitmap: Bitmap, into: RectF, contain: Boolean) {
        val bw = bitmap.width.toFloat()
        val bh = bitmap.height.toFloat()
        if (bw <= 0f || bh <= 0f) return
        if (contain) {
            val scale = min(into.width() / bw, into.height() / bh)
            val w = bw * scale
            val h = bh * scale
            area.set(into.centerX() - w / 2f, into.centerY() - h / 2f, into.centerX() + w / 2f, into.centerY() + h / 2f)
            canvas.drawBitmap(bitmap, null, area, bitmapPaint)
        } else {
            val scale = max(into.width() / bw, into.height() / bh)
            val w = into.width() / scale
            val h = into.height() / scale
            val left = ((bw - w) / 2f).toInt()
            val top = ((bh - h) / 2f).toInt()
            src.set(left, top, left + w.toInt(), top + h.toInt())
            canvas.drawBitmap(bitmap, src, into, bitmapPaint)
        }
    }

    // ---- Elements ----------------------------------------------------------

    /**
     * Draw every element of `face` in `frame`. `values` are the agent's, by
     * index; `fallbackTitle` is what an empty title of a schema-3 face shows.
     * Returns how many milliseconds until it needs drawing again for its own
     * sake (a clock ticking, something pulsing), or `null` for never.
     */
    fun elements(
        canvas: Canvas,
        key: String,
        frame: RectF,
        face: FaceSpec,
        places: Map<String, Box>,
        values: Map<Int, ElementLive>,
        legacyLabel: String,
        legacyIcon: String,
        ink: Int,
        series: Map<String, List<Double>>,
        resolve: (String) -> String,
        nowMs: Long,
        fallbackTitle: String,
    ): Long? {
        var due: Long? = null
        fun soon(ms: Long) {
            due = min(due ?: ms, ms)
        }
        for ((index, element) in face.elements.withIndex()) {
            val place = (if (element.box != null) places["#$index"] else places[element.slot]) ?: continue
            val live = values[index]
            val box = slot
            box.set(
                frame.left + place.x.toFloat(),
                frame.top + place.y.toFloat(),
                frame.left + place.right.toFloat(),
                frame.top + place.bottom.toFloat(),
            )
            var color = parse(live?.color) ?: parse(element.color) ?: ink
            if (live?.pulse == true) {
                color = withAlpha(color, pulse(nowMs))
                soon(PULSE_FRAME_MS)
            }
            val style = FaceLayout.style(face.layout, element.slot)
            when (element.kind) {
                "ELEMENT_KIND_ICON" -> {
                    val value = when {
                        !element.live -> element.template
                        live != null -> live.text
                        else -> legacyIcon
                    }
                    val side = min(box.width(), box.height())
                    icon(canvas, value.ifBlank { null }, box.centerX() - side / 2f, box.centerY() - side / 2f, side, color)
                }
                "ELEMENT_KIND_BAR" -> bar(canvas, box, fraction(live?.number, element), color)
                "ELEMENT_KIND_GAUGE" -> gauge(canvas, box, fraction(live?.number, element), color)
                "ELEMENT_KIND_GRAPH" -> {
                    val points = series[resolve(element.variable)]
                    if (points != null && points.size >= 2) {
                        graph(canvas, box, points, element.min, element.max, element.graphStyle, element.points, color, faint = false)
                    }
                }
                "ELEMENT_KIND_TIMER" -> {
                    val running = live?.text == "run"
                    val left = Clock.timerLeft(live?.number, running, live?.receivedAt ?: nowMs, nowMs)
                    words(canvas, "$key#$index", Clock.countdown(left), box, element.copy(font = if (element.font == "FONT_SANS") "FONT_MONO" else element.font), style.copy(size = if (element.size == "TEXT_SIZE_AUTO") "TEXT_SIZE_FIT" else style.size), color)
                    // Next when the shown second changes.
                    if (running && left != null && left > 0) soon(((left * 1000).toLong() % 1000).let { if (it == 0L) 1000L else it })
                }
                "ELEMENT_KIND_CLOCK" -> {
                    val zone = Clock.zone(element.zone)
                    if (element.analog) {
                        analog(canvas, box, nowMs, zone, element.seconds, color)
                        soon(if (element.seconds) 1000 - nowMs % 1000 else 60_000 - nowMs % 60_000)
                    } else {
                        val now = Clock.text(nowMs, element.format, zone, element.seconds)
                        words(canvas, "$key#$index", now, box, element.copy(font = if (element.font == "FONT_SANS") "FONT_MONO" else element.font), style.copy(size = if (element.size == "TEXT_SIZE_AUTO") "TEXT_SIZE_FIT" else style.size), color)
                        soon(if (element.seconds || element.format.contains("%S")) 1000 - nowMs % 1000 else 60_000 - nowMs % 60_000)
                    }
                }
                else -> {
                    var value = when {
                        !element.live -> element.template
                        live != null -> live.text
                        // An older agent sends only the label.
                        element.slot == "SLOT_TITLE" -> legacyLabel
                        else -> ""
                    }
                    if (value.isBlank() && element.slot == "SLOT_TITLE") value = fallbackTitle
                    if (element.slot == "SLOT_BADGE") {
                        if (value.isNotBlank()) badge(canvas, "$key#$index", value, box, element, style, color)
                    } else if (element.marquee && marquee(canvas, "$key#$index", value, box, element, style, if (style.quiet) withAlpha(color, 0.66f) else color, nowMs)) {
                        soon(MARQUEE_FRAME_MS)
                    } else {
                        words(canvas, "$key#$index", value, box, element, style, if (style.quiet) withAlpha(color, 0.66f) else color)
                    }
                }
            }
        }
        return due
    }

    /** 0–1 along an element's range, or `null` while its value is unknown. */
    private fun fraction(value: Double?, element: ElementSpec): Float? {
        value ?: return null
        val (low, high) = if (element.min == 0.0 && element.max == 0.0) 0.0 to 100.0 else element.min to element.max
        if (high <= low) return null
        return ((value - low) / (high - low)).coerceIn(0.0, 1.0).toFloat()
    }

    private fun bar(canvas: Canvas, box: RectF, fraction: Float?, color: Int) {
        val thickness = max(4f * unit, min(box.height(), box.width()) * 0.08f).coerceAtMost(box.height())
        area.set(box.left, box.bottom - thickness, box.right, box.bottom)
        fill.color = TRACK
        canvas.drawRoundRect(area, thickness / 2f, thickness / 2f, fill)
        if (fraction != null && fraction > 0f) {
            area.right = box.left + box.width() * fraction
            fill.color = color
            canvas.drawRoundRect(area, thickness / 2f, thickness / 2f, fill)
        }
    }

    private fun gauge(canvas: Canvas, box: RectF, fraction: Float?, color: Int) {
        val side = min(box.width(), box.height())
        val width = side * 0.1f
        area.set(box.centerX() - side / 2f + width / 2f, box.centerY() - side / 2f + width / 2f, box.centerX() + side / 2f - width / 2f, box.centerY() + side / 2f - width / 2f)
        stroke.strokeWidth = width
        stroke.color = TRACK
        canvas.drawArc(area, 135f, 270f, false, stroke)
        if (fraction != null && fraction > 0f) {
            stroke.color = color
            canvas.drawArc(area, 135f, 270f * fraction, false, stroke)
        }
    }

    private fun graph(
        canvas: Canvas,
        box: RectF,
        all: List<Double>,
        min: Double,
        max: Double,
        style: String,
        count: Int,
        color: Int,
        faint: Boolean,
    ) {
        val points = all.takeLast(if (count > 0) count else 60)
        if (points.size < 2) return
        val (low, high) = if (min == 0.0 && max == 0.0) {
            val lo = points.min()
            val hi = points.max()
            // A flat line sits in the middle rather than on an edge.
            if (hi - lo < 1e-9) (lo - 1) to (hi + 1) else lo to hi
        } else {
            min to max
        }
        val span = (high - low).takeIf { it > 0 } ?: 1.0
        fun x(i: Int) = box.left + box.width() * i / (points.size - 1).coerceAtLeast(1)
        fun y(v: Double) = box.bottom - box.height() * ((v - low) / span).coerceIn(0.0, 1.0).toFloat()

        if (style == "GRAPH_STYLE_BARS") {
            val w = box.width() / points.size
            fill.color = withAlpha(color, if (faint) 0.3f else 0.9f)
            for ((i, v) in points.withIndex()) {
                area.set(box.left + i * w + w * 0.15f, y(v), box.left + (i + 1) * w - w * 0.15f, box.bottom)
                canvas.drawRect(area, fill)
            }
            return
        }
        graphPath.reset()
        graphPath.moveTo(x(0), y(points[0]))
        for (i in 1 until points.size) graphPath.lineTo(x(i), y(points[i]))
        if (style == "GRAPH_STYLE_AREA") {
            graphPath.lineTo(box.right, box.bottom)
            graphPath.lineTo(box.left, box.bottom)
            graphPath.close()
            fill.shader = LinearGradient(0f, box.top, 0f, box.bottom, withAlpha(color, if (faint) 0.35f else 0.45f), withAlpha(color, 0f), Shader.TileMode.CLAMP)
            canvas.drawPath(graphPath, fill)
            fill.shader = null
            graphPath.reset()
            graphPath.moveTo(x(0), y(points[0]))
            for (i in 1 until points.size) graphPath.lineTo(x(i), y(points[i]))
        }
        stroke.color = withAlpha(color, if (faint) 0.55f else 1f)
        stroke.strokeWidth = 2f * unit
        canvas.drawPath(graphPath, stroke)
    }

    private fun analog(canvas: Canvas, box: RectF, nowMs: Long, zone: java.util.TimeZone, seconds: Boolean, color: Int) {
        val side = min(box.width(), box.height())
        val r = side / 2f
        val cx = box.centerX()
        val cy = box.centerY()
        stroke.color = withAlpha(color, 0.35f)
        stroke.strokeWidth = side * 0.025f
        canvas.drawCircle(cx, cy, r - stroke.strokeWidth, stroke)
        stroke.color = withAlpha(color, 0.7f)
        for (tick in 0 until 12) {
            val angle = Math.toRadians(tick * 30.0)
            val outer = r * 0.86f
            val inner = if (tick % 3 == 0) r * 0.7f else r * 0.78f
            canvas.drawLine(cx + (sin(angle) * inner).toFloat(), cy - (cos(angle) * inner).toFloat(), cx + (sin(angle) * outer).toFloat(), cy - (cos(angle) * outer).toFloat(), stroke)
        }
        val (hour, minute, second) = Clock.hands(nowMs, zone)
        fun hand(degrees: Float, length: Float, width: Float, paint: Int) {
            val angle = Math.toRadians(degrees.toDouble())
            stroke.color = paint
            stroke.strokeWidth = width
            canvas.drawLine(cx, cy, cx + (sin(angle) * length).toFloat(), cy - (cos(angle) * length).toFloat(), stroke)
        }
        hand(hour, r * 0.5f, side * 0.06f, color)
        hand(minute, r * 0.74f, side * 0.04f, color)
        if (seconds) hand(second, r * 0.8f, side * 0.015f, SIGNAL)
        fill.color = color
        canvas.drawCircle(cx, cy, side * 0.04f, fill)
    }

    // ---- Text ----------------------------------------------------------------

    private fun typeface(element: ElementSpec, bold: Boolean): Typeface {
        val family = when (element.font) {
            "FONT_MONO" -> Typeface.MONOSPACE
            "FONT_SERIF" -> Typeface.SERIF
            else -> Typeface.SANS_SERIF
        }
        return Typeface.create(family, if (bold) Typeface.BOLD else Typeface.NORMAL)
    }

    /** Wrap and size `value` for `box`, or reuse what was worked out last time. */
    private fun layout(key: String, value: String, box: Box, element: ElementSpec, style: dev.flexdeck.client.PlaceStyle): Laid {
        val bold = element.bold ?: style.bold
        val face = typeface(element, bold)
        val maxLines = if (element.maxLines > 0) element.maxLines else style.maxLines
        val size = if (element.size == "TEXT_SIZE_AUTO") style.size else element.size
        laid[key]?.let { if (it.source == value && it.box == box && it.typeface == face && it.maxLines == maxLines) return it }

        text.typeface = face
        val width = box.w.toFloat()
        val px = if (size == "TEXT_SIZE_FIT") {
            // As large as fits: the height first, then shrink until it fits across.
            var candidate = (box.h * 0.8).toFloat()
            text.textSize = candidate
            val measured = text.measureText(value)
            if (measured > width && measured > 0f) candidate *= width / measured
            candidate.coerceAtLeast(8f * unit)
        } else {
            FaceLayout.textPx(size).toFloat() * unit
        }
        text.textSize = px
        val lines = wrap(value, width, maxLines)
        return Laid(value, box, px, face, maxLines, lines).also { laid[key] = it }
    }

    /** Words onto at most `maxLines` lines of `width`, the last cut with an ellipsis. */
    private fun wrap(value: String, width: Float, maxLines: Int): List<String> {
        if (width <= 0f || value.isEmpty()) return emptyList()
        val lines = ArrayList<String>()
        var rest = value.trim()
        while (rest.isNotEmpty() && lines.size < maxLines) {
            val last = lines.size == maxLines - 1
            if (text.measureText(rest) <= width) {
                lines += rest
                break
            }
            if (last) {
                val count = text.breakText(rest, true, width - text.measureText("…"), null)
                lines += if (count <= 0) "…" else rest.substring(0, count).trimEnd() + "…"
                break
            }
            val count = text.breakText(rest, true, width, null).coerceAtLeast(1)
            // Break at the last space that fits, if there is one.
            val space = rest.lastIndexOf(' ', count).takeIf { it > 0 } ?: count
            lines += rest.substring(0, space).trimEnd()
            rest = rest.substring(space).trimStart()
        }
        return lines
    }

    private fun words(canvas: Canvas, key: String, value: String, box: RectF, element: ElementSpec, style: dev.flexdeck.client.PlaceStyle, color: Int) {
        if (value.isEmpty()) return
        val place = Box(0.0, 0.0, box.width().toDouble(), box.height().toDouble())
        val laid = layout(key, value, place, element, style)
        if (laid.lines.isEmpty()) return
        text.typeface = laid.typeface
        text.textSize = laid.px
        text.color = color
        val align = if (element.align == "TEXT_ALIGN_AUTO") style.align else element.align
        text.textAlign = when (align) {
            "TEXT_ALIGN_START" -> Paint.Align.LEFT
            "TEXT_ALIGN_END" -> Paint.Align.RIGHT
            else -> Paint.Align.CENTER
        }
        val x = when (text.textAlign) {
            Paint.Align.LEFT -> box.left
            Paint.Align.RIGHT -> box.right
            else -> box.centerX()
        }
        val metrics = text.fontMetrics
        val line = metrics.descent - metrics.ascent
        val total = line * laid.lines.size
        var baseline = box.centerY() - total / 2f - metrics.ascent
        for (row in laid.lines) {
            canvas.drawText(row, x, baseline, text)
            baseline += line
        }
    }

    /**
     * One line that scrolls sideways when it is wider than its place, with a
     * gap before it comes round again. Returns false — drawing nothing — when
     * it fits, for it to be drawn as still words.
     */
    private fun marquee(canvas: Canvas, key: String, value: String, box: RectF, element: ElementSpec, style: dev.flexdeck.client.PlaceStyle, color: Int, nowMs: Long): Boolean {
        if (value.isEmpty()) return false
        val one = element.copy(maxLines = 1, size = if (element.size == "TEXT_SIZE_FIT") "TEXT_SIZE_AUTO" else element.size)
        val laid = layout("$key~", value, Box(0.0, 0.0, box.width().toDouble(), box.height().toDouble()), one, style.copy(maxLines = 1))
        text.typeface = laid.typeface
        text.textSize = laid.px
        val wide = text.measureText(value)
        if (wide <= box.width()) return false
        text.color = color
        text.textAlign = Paint.Align.LEFT
        val gap = box.width() * 0.35f
        val period = wide + gap
        val offset = dev.flexdeck.client.Clock.scrolled(nowMs, (MARQUEE_PX_PER_S * unit).toDouble(), period.toDouble()).toFloat()
        val metrics = text.fontMetrics
        val baseline = box.centerY() - (metrics.descent + metrics.ascent) / 2f
        canvas.save()
        canvas.clipRect(box)
        canvas.drawText(value, box.left - offset, baseline, text)
        canvas.drawText(value, box.left - offset + period, baseline, text)
        canvas.restore()
        return true
    }

    private fun badge(canvas: Canvas, key: String, value: String, box: RectF, element: ElementSpec, style: dev.flexdeck.client.PlaceStyle, color: Int) {
        fill.color = SIGNAL
        val r = box.height() / 2f
        canvas.drawRoundRect(box, r, r, fill)
        words(canvas, key, value, box, element, style, if (parse(element.color) == null) Color.WHITE else color)
    }

    fun pulse(nowMs: Long): Float = 0.55f + 0.45f * ((sin(nowMs / 260.0) + 1.0) / 2.0).toFloat()

    private fun withAlpha(color: Int, alpha: Float): Int =
        Color.argb((Color.alpha(color) * alpha).toInt().coerceIn(0, 255), Color.red(color), Color.green(color), Color.blue(color))

    companion object {
        const val PULSE_FRAME_MS = 16L
        /** A marquee moves smoothly enough at 30 frames a second, for half the work. */
        const val MARQUEE_FRAME_MS = 33L
        const val MARQUEE_PX_PER_S = 36f
        val SIGNAL = Color.parseColor("#FF5A1F")
        val TRACK = Color.parseColor("#2A2A2F")
    }
}
