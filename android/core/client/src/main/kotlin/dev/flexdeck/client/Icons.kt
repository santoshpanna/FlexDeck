package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.math.tan

/**
 * The bundled icons, read from the `icons.json` asset the portal's icon set is
 * exported to — so the tablet draws exactly what the editor offered.
 *
 * Paths are SVG path data on a 24×24 grid. They are turned into moves, lines
 * and cubic curves here, in plain Kotlin, so the drawing code needs nothing but
 * `android.graphics.Path` — and so the parsing is tested without a device.
 */
object Icons {

    /** `{name: [path data, …]}` from the asset's text. */
    fun read(text: String): Map<String, List<String>> {
        val root = Json.parseToJsonElement(text) as? JsonObject ?: return emptyMap()
        return root.mapValues { (_, paths) ->
            (paths as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.content } ?: emptyList()
        }
    }

    /** What a widget's icon value is: a bundled name, a picture to fetch, or a pack's icon. */
    sealed interface Source {
        data class Bundled(val name: String) : Source
        data class Image(val sha256: String) : Source
        /** `pack:<pack>/<name>`: an icon from an installed icon pack. */
        data class Pack(val pack: String, val name: String) : Source
    }

    fun source(value: String?): Source? {
        val text = value?.trim().orEmpty()
        if (text.isEmpty()) return null
        if (text.startsWith("img:")) {
            val sha = text.removePrefix("img:")
            return if (sha.length == 64 && sha.all { it in '0'..'9' || it in 'a'..'f' }) Source.Image(sha) else null
        }
        if (text.startsWith("pack:")) {
            val (pack, name) = text.removePrefix("pack:").split('/', limit = 2).let { it.getOrNull(0).orEmpty() to it.getOrNull(1).orEmpty() }
            return if (pack.isNotEmpty() && name.isNotEmpty()) Source.Pack(pack, name) else null
        }
        return Source.Bundled(text)
    }

    /** One icon of a pack: a picture, or paths on a view box. */
    data class PackIcon(
        val image: String? = null,
        val paths: List<String> = emptyList(),
        /** minX, minY, width, height. */
        val view: List<Float> = listOf(0f, 0f, 24f, 24f),
        val fill: Boolean = false,
        val stroke: Float? = null,
    )

    /** A pack, as `GET /api/iconpacks/{id}` answers: its icons by name. */
    fun readPack(text: String): Map<String, PackIcon> {
        val root = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: return emptyMap()
        return (root["icons"] as? JsonArray).orEmpty().mapNotNull { item ->
            val icon = item as? JsonObject ?: return@mapNotNull null
            val name = (icon["name"] as? JsonPrimitive)?.content ?: return@mapNotNull null
            val view = (icon["view"] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.content?.toFloatOrNull() }
                ?.takeIf { it.size == 4 && it[2] > 0f && it[3] > 0f }
            name to PackIcon(
                image = (icon["image"] as? JsonPrimitive)?.content,
                paths = (icon["paths"] as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.content },
                view = view ?: listOf(0f, 0f, 24f, 24f),
                fill = (icon["fill"] as? JsonPrimitive)?.content == "true",
                stroke = (icon["stroke"] as? JsonPrimitive)?.content?.toFloatOrNull(),
            )
        }.toMap()
    }
}

/** One drawing step, in absolute coordinates. */
sealed interface Segment {
    data class Move(val x: Float, val y: Float) : Segment
    data class Line(val x: Float, val y: Float) : Segment
    data class Cubic(
        val x1: Float, val y1: Float,
        val x2: Float, val y2: Float,
        val x: Float, val y: Float,
    ) : Segment
    data object Close : Segment
}

/**
 * SVG path data as [Segment]s: M, L, H, V, C, A and Z, absolute and relative.
 * Arcs become cubic curves. Something it does not understand ends the path
 * there rather than drawing garbage.
 */
object PathData {

    fun parse(data: String): List<Segment> {
        val tokens = tokenize(data)
        val out = mutableListOf<Segment>()
        var i = 0
        var command = ' '
        var x = 0f
        var y = 0f
        var startX = 0f
        var startY = 0f
        // The last curve's control point, for S and T to mirror.
        var controlX = 0f
        var controlY = 0f
        var previousCurve = ' '

        fun number(): Float? = (tokens.getOrNull(i) as? Float)?.also { i++ }
        fun flag(): Boolean? = number()?.let { it != 0f }

        while (i < tokens.size) {
            val token = tokens[i]
            if (token is Char) {
                command = token
                i++
                if (command == 'Z' || command == 'z') {
                    out += Segment.Close
                    x = startX
                    y = startY
                    continue
                }
            } else if (command == ' ') {
                break
            }
            val relative = command.isLowerCase()
            val ox = if (relative) x else 0f
            val oy = if (relative) y else 0f
            when (command.uppercaseChar()) {
                'M' -> {
                    val px = number() ?: break
                    val py = number() ?: break
                    x = ox + px; y = oy + py
                    startX = x; startY = y
                    out += Segment.Move(x, y)
                    // Further pairs after a move are lines.
                    command = if (relative) 'l' else 'L'
                }
                'L' -> {
                    val px = number() ?: break
                    val py = number() ?: break
                    x = ox + px; y = oy + py
                    out += Segment.Line(x, y)
                }
                'H' -> {
                    val px = number() ?: break
                    x = ox + px
                    out += Segment.Line(x, y)
                }
                'V' -> {
                    val py = number() ?: break
                    y = oy + py
                    out += Segment.Line(x, y)
                }
                'C' -> {
                    val values = List(6) { number() ?: return out }
                    out += Segment.Cubic(
                        ox + values[0], oy + values[1],
                        ox + values[2], oy + values[3],
                        ox + values[4], oy + values[5],
                    )
                    controlX = ox + values[2]; controlY = oy + values[3]
                    x = ox + values[4]; y = oy + values[5]
                    previousCurve = 'C'
                    continue
                }
                'S' -> {
                    val values = List(4) { number() ?: return out }
                    // The first control point mirrors the last curve's second.
                    val c1x = if (previousCurve == 'C') 2 * x - controlX else x
                    val c1y = if (previousCurve == 'C') 2 * y - controlY else y
                    out += Segment.Cubic(c1x, c1y, ox + values[0], oy + values[1], ox + values[2], oy + values[3])
                    controlX = ox + values[0]; controlY = oy + values[1]
                    x = ox + values[2]; y = oy + values[3]
                    previousCurve = 'C'
                    continue
                }
                'Q', 'T' -> {
                    val qx: Float
                    val qy: Float
                    if (command.uppercaseChar() == 'Q') {
                        qx = ox + (number() ?: break); qy = oy + (number() ?: break)
                    } else {
                        qx = if (previousCurve == 'Q') 2 * x - controlX else x
                        qy = if (previousCurve == 'Q') 2 * y - controlY else y
                    }
                    val px = ox + (number() ?: break)
                    val py = oy + (number() ?: break)
                    // A quadratic is the cubic with its controls two thirds of the way to its one.
                    out += Segment.Cubic(
                        x + 2f / 3f * (qx - x), y + 2f / 3f * (qy - y),
                        px + 2f / 3f * (qx - px), py + 2f / 3f * (qy - py),
                        px, py,
                    )
                    controlX = qx; controlY = qy
                    x = px; y = py
                    previousCurve = 'Q'
                    continue
                }
                'A' -> {
                    val rx = number() ?: break
                    val ry = number() ?: break
                    val rotation = number() ?: break
                    val large = flag() ?: break
                    val sweep = flag() ?: break
                    val px = number() ?: break
                    val py = number() ?: break
                    out += arc(x, y, rx, ry, rotation, large, sweep, ox + px, oy + py)
                    x = ox + px; y = oy + py
                }
                else -> break
            }
            // Anything but a curve leaves nothing for S or T to mirror.
            previousCurve = ' '
        }
        return out
    }

    /** Letters and numbers, with "1.5.5" and "-1-2" split the way SVG reads them. */
    private fun tokenize(data: String): List<Any> {
        val tokens = mutableListOf<Any>()
        val number = StringBuilder()
        var sawDot = false
        fun flush() {
            if (number.isNotEmpty()) {
                number.toString().toFloatOrNull()?.let { tokens += it }
                number.clear()
            }
            sawDot = false
        }
        var previous = ' '
        for (c in data) {
            when {
                c.isLetter() && c != 'e' && c != 'E' -> {
                    flush(); tokens += c
                }
                c == '-' && !(previous == 'e' || previous == 'E') -> {
                    flush(); number.append(c)
                }
                c == '.' -> {
                    if (sawDot) flush()
                    number.append(c); sawDot = true
                }
                c.isDigit() || c == 'e' || c == 'E' || c == '-' -> number.append(c)
                else -> flush()
            }
            previous = c
        }
        flush()
        return tokens
    }

    /**
     * An SVG elliptical arc as cubic curves: endpoint form to centre form (SVG
     * spec, appendix F.6), then one cubic per quarter turn or less.
     */
    internal fun arc(
        x1: Float, y1: Float,
        rxIn: Float, ryIn: Float,
        rotationDegrees: Float,
        large: Boolean, sweep: Boolean,
        x2: Float, y2: Float,
    ): List<Segment> {
        if (x1 == x2 && y1 == y2) return emptyList()
        var rx = abs(rxIn.toDouble())
        var ry = abs(ryIn.toDouble())
        if (rx == 0.0 || ry == 0.0) return listOf(Segment.Line(x2, y2))
        val phi = rotationDegrees * PI / 180
        val cosPhi = cos(phi)
        val sinPhi = sin(phi)
        val dx = (x1 - x2) / 2.0
        val dy = (y1 - y2) / 2.0
        val x1p = cosPhi * dx + sinPhi * dy
        val y1p = -sinPhi * dx + cosPhi * dy
        val lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry)
        if (lambda > 1) {
            val scale = sqrt(lambda)
            rx *= scale; ry *= scale
        }
        val sign = if (large == sweep) -1 else 1
        val numerator = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p
        val denominator = rx * rx * y1p * y1p + ry * ry * x1p * x1p
        val coefficient = sign * sqrt(maxOf(0.0, numerator / denominator))
        val cxp = coefficient * rx * y1p / ry
        val cyp = -coefficient * ry * x1p / rx
        val cx = cosPhi * cxp - sinPhi * cyp + (x1 + x2) / 2.0
        val cy = sinPhi * cxp + cosPhi * cyp + (y1 + y2) / 2.0

        fun angle(ux: Double, uy: Double, vx: Double, vy: Double): Double =
            atan2(ux * vy - uy * vx, ux * vx + uy * vy)
        val theta1 = angle(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry)
        var delta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
        if (!sweep && delta > 0) delta -= 2 * PI
        if (sweep && delta < 0) delta += 2 * PI

        val pieces = ceil(abs(delta) / (PI / 2)).toInt().coerceAtLeast(1)
        val step = delta / pieces
        val k = 4.0 / 3.0 * tan(step / 4)
        val out = mutableListOf<Segment>()
        var theta = theta1
        fun point(t: Double): Pair<Double, Double> =
            (cx + rx * cos(t) * cosPhi - ry * sin(t) * sinPhi) to (cy + rx * cos(t) * sinPhi + ry * sin(t) * cosPhi)
        fun derivative(t: Double): Pair<Double, Double> =
            (-rx * sin(t) * cosPhi - ry * cos(t) * sinPhi) to (-rx * sin(t) * sinPhi + ry * cos(t) * cosPhi)
        repeat(pieces) { index ->
            val next = theta + step
            val (sx, sy) = point(theta)
            val (ex, ey) = if (index == pieces - 1) x2.toDouble() to y2.toDouble() else point(next)
            val (dsx, dsy) = derivative(theta)
            val (dex, dey) = derivative(next)
            out += Segment.Cubic(
                (sx + k * dsx).toFloat(), (sy + k * dsy).toFloat(),
                (ex - k * dex).toFloat(), (ey - k * dey).toFloat(),
                ex.toFloat(), ey.toFloat(),
            )
            theta = next
        }
        return out
    }
}
