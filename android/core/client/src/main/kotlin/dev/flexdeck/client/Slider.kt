package dev.flexdeck.client

/** Which way a slider runs. AUTO follows the widget's longer side. */
enum class SliderOrientation { AUTO, HORIZONTAL, VERTICAL }

/** A slider's range, the step its value snaps to (0: none), and its direction. */
data class SliderSpec(
    val min: Double = 0.0,
    val max: Double = 100.0,
    val step: Double = 0.0,
    val orientation: SliderOrientation = SliderOrientation.AUTO,
)

/**
 * Where a finger on a slider is, as a value — and back.
 *
 * Pure arithmetic, kept out of the view so it can be tested without a screen:
 * this is where "the thumb is not under my finger" bugs live. A vertical
 * slider's bottom is its minimum, the way a fader works.
 */
object SliderMath {

    /** Whether a slider of this size runs vertically. */
    fun isVertical(spec: SliderSpec, width: Float, height: Float): Boolean = when (spec.orientation) {
        SliderOrientation.VERTICAL -> true
        SliderOrientation.HORIZONTAL -> false
        SliderOrientation.AUTO -> height > width
    }

    /** Bring a value into range and onto a step. */
    fun settle(spec: SliderSpec, value: Double): Double {
        val clamped = if (value.isNaN()) spec.min else value.coerceIn(spec.min, spec.max)
        if (spec.step <= 0.0) return clamped
        val steps = Math.round((clamped - spec.min) / spec.step)
        return (spec.min + steps * spec.step).coerceIn(spec.min, spec.max)
    }

    /** How far along its track a value sits, 0 to 1. */
    fun fraction(spec: SliderSpec, value: Double): Float =
        ((settle(spec, value) - spec.min) / (spec.max - spec.min)).toFloat().coerceIn(0f, 1f)

    /**
     * The value under a finger at (`x`, `y`), measured from the slider's
     * top-left, for a slider `width` by `height`. A finger past either end
     * holds the end value, so dragging off the widget pins it rather than
     * jumping.
     */
    fun valueAt(spec: SliderSpec, width: Float, height: Float, x: Float, y: Float): Double {
        val along = if (isVertical(spec, width, height)) {
            if (height <= 0f) 0f else 1f - (y / height)
        } else {
            if (width <= 0f) 0f else x / width
        }
        val fraction = along.coerceIn(0f, 1f).toDouble()
        return settle(spec, spec.min + fraction * (spec.max - spec.min))
    }

    /**
     * The value after a finger that landed at `startPos` (when the slider read
     * `startValue`) has moved to `pos`, along a track `length` long. Relative,
     * the way a fader is: grabbing the tile anywhere never makes the value
     * jump, and only the distance moved changes it. Positions are measured
     * down for a vertical slider and across for a horizontal one; up and right
     * are louder.
     */
    fun dragValue(
        spec: SliderSpec,
        vertical: Boolean,
        startValue: Double,
        startPos: Float,
        pos: Float,
        length: Float,
    ): Double {
        if (length <= 0f) return settle(spec, startValue)
        val moved = if (vertical) startPos - pos else pos - startPos
        return settle(spec, startValue + (moved / length) * (spec.max - spec.min))
    }
}

/**
 * Paces a drag's values to the agent: at most one per [intervalMs], the
 * newest wins, and the value a finger lifts on is always sent.
 *
 * A finger moves at the screen's refresh rate; the agent only needs enough to
 * feel live. Sending every frame would queue work the agent then has to throw
 * away. Time is passed in rather than read, so this is testable to the
 * millisecond.
 */
class ValuePacer(private val intervalMs: Long = 33) {
    private var lastSentAt: Long = Long.MIN_VALUE / 2
    private var lastSent: Double? = null
    private var pending: Double? = null

    /** The finger moved to [value]. Returns the value to send now, if any. */
    fun move(value: Double, nowMs: Long): Double? {
        if (value == lastSent && pending == null) return null
        return if (nowMs - lastSentAt >= intervalMs) {
            sent(value, nowMs)
        } else {
            pending = value
            null
        }
    }

    /** How long until the held-back value is due, or `null` when there is none. */
    fun dueInMs(nowMs: Long): Long? =
        pending?.let { (intervalMs - (nowMs - lastSentAt)).coerceAtLeast(0) }

    /** Called when [dueInMs] has passed: the held-back value, if still due. */
    fun flush(nowMs: Long): Double? {
        val value = pending ?: return null
        if (nowMs - lastSentAt < intervalMs) return null
        return sent(value, nowMs)
    }

    /** The finger lifted on [value]: always sent, and the pacer starts over. */
    fun release(value: Double, nowMs: Long): Double {
        sent(value, nowMs)
        lastSent = null
        return value
    }

    private fun sent(value: Double, nowMs: Long): Double {
        lastSentAt = nowMs
        lastSent = value
        pending = null
        return value
    }
}
