package dev.flexdeck.client

import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import java.util.TimeZone

/**
 * A clock element's time, from the tablet's own clock.
 *
 * Formats are written strftime-style, as the agent's `time` filter takes them
 * (`%H:%M`), and turned into `SimpleDateFormat` patterns here — `java.time`
 * needs API 26 and this app runs from 21.
 */
object Clock {

    /** A countdown as a key shows it: `4:07`, `1:02:00` — whole seconds, rounded up. */
    fun countdown(seconds: Double?): String {
        if (seconds == null || !seconds.isFinite()) return "\u2013:\u2013\u2013"
        val total = kotlin.math.max(0L, kotlin.math.ceil(seconds - 1e-9).toLong())
        val h = total / 3600
        val m = (total % 3600) / 60
        val s = total % 60
        val ss = s.toString().padStart(2, '0')
        return if (h > 0) "$h:${m.toString().padStart(2, '0')}:$ss" else "$m:$ss"
    }

    /** Seconds left at `nowMs`, from `left` sent at `sentAtMs`, counting down while it runs. */
    fun timerLeft(left: Double?, running: Boolean, sentAtMs: Long, nowMs: Long): Double? =
        left?.let { if (running) kotlin.math.max(0.0, it - (nowMs - sentAtMs) / 1000.0) else it }

    /**
     * How far a marquee moving at `pxPerS` has scrolled by `nowMs`, wrapped to
     * `period`. In doubles: a float holds today's milliseconds only to the
     * nearest two minutes, and the words would stand still.
     */
    fun scrolled(nowMs: Long, pxPerS: Double, period: Double): Double =
        if (period > 0.0) (nowMs * pxPerS / 1000.0) % period else 0.0

    /** `%H:%M` → `HH:mm`. Literal text is quoted; an unknown `%x` is kept as text. */
    fun pattern(strftime: String): String {
        val out = StringBuilder()
        val literal = StringBuilder()
        fun flush() {
            if (literal.isNotEmpty()) {
                out.append('\'').append(literal.toString().replace("'", "''")).append('\'')
                literal.clear()
            }
        }
        var i = 0
        while (i < strftime.length) {
            val c = strftime[i]
            if (c != '%' || i + 1 >= strftime.length) {
                literal.append(c)
                i++
                continue
            }
            val padless = strftime[i + 1] == '-' && i + 2 < strftime.length
            val code = if (padless) strftime[i + 2] else strftime[i + 1]
            val mapped = when (code) {
                'H' -> if (padless) "H" else "HH"
                'I' -> if (padless) "h" else "hh"
                'M' -> "mm"
                'S' -> "ss"
                'p' -> "a"
                'a' -> "EEE"
                'A' -> "EEEE"
                'd' -> if (padless) "d" else "dd"
                'e' -> "d"
                'm' -> if (padless) "M" else "MM"
                'b' -> "MMM"
                'B' -> "MMMM"
                'y' -> "yy"
                'Y' -> "yyyy"
                'Z' -> "zzz"
                else -> null
            }
            when {
                code == '%' -> literal.append('%')
                mapped == null -> literal.append('%').append(code)
                else -> {
                    flush()
                    out.append(mapped)
                }
            }
            i += if (padless) 3 else 2
        }
        flush()
        return out.toString()
    }

    /** The zone named, or the tablet's own for an empty or unknown name. */
    fun zone(name: String): TimeZone {
        if (name.isBlank()) return TimeZone.getDefault()
        val zone = TimeZone.getTimeZone(name.trim())
        // An unknown id comes back as GMT: take the tablet's rather than a wrong hour.
        return if (zone.id == "GMT" && !name.trim().equals("GMT", ignoreCase = true)) TimeZone.getDefault() else zone
    }

    /** `now` as text in `zone`; an empty format is "%H:%M", or "%H:%M:%S" with seconds. */
    fun text(nowMs: Long, format: String, zone: TimeZone, seconds: Boolean, locale: Locale = Locale.getDefault()): String {
        val strftime = format.ifBlank { if (seconds) "%H:%M:%S" else "%H:%M" }
        val formatter = SimpleDateFormat(pattern(strftime), locale)
        formatter.timeZone = zone
        return formatter.format(Date(nowMs))
    }

    /** Hour, minute and second hand angles in degrees clockwise from 12. */
    fun hands(nowMs: Long, zone: TimeZone): Triple<Float, Float, Float> {
        val calendar = Calendar.getInstance(zone).apply { timeInMillis = nowMs }
        val seconds = calendar.get(Calendar.SECOND) + calendar.get(Calendar.MILLISECOND) / 1000f
        val minutes = calendar.get(Calendar.MINUTE) + seconds / 60f
        val hours = calendar.get(Calendar.HOUR) + minutes / 60f
        return Triple(hours * 30f, minutes * 6f, seconds * 6f)
    }
}
