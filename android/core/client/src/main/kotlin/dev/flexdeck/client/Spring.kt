package dev.flexdeck.client

import kotlin.math.abs
import kotlin.math.min

/**
 * A damped spring, integrated by hand: the motion behind every key press,
 * thumb and fade on the deck.
 *
 * The same feel as the editor's `spring.key` — stiff, a hint of overshoot —
 * without an animation library on a tablet with a gigabyte of RAM. The view
 * steps it once per frame and stops asking for frames once [settled].
 *
 * Pure and clock-free: time comes in through [step], so it is testable.
 */
class Spring(
    initial: Float,
    private val stiffness: Float = 700f,
    private val damping: Float = 30f,
) {
    var value: Float = initial
        private set
    var target: Float = initial
    private var velocity = 0f

    /** Whether it has come to rest on its target. */
    val settled: Boolean
        get() = abs(target - value) < EPSILON && abs(velocity) < EPSILON

    /** Jump straight to `to`, with no motion: for the first frame, or reduced motion. */
    fun snap(to: Float) {
        value = to
        target = to
        velocity = 0f
    }

    /**
     * Advance by `seconds` (clamped, so a stalled frame does not fling it) in
     * small fixed sub-steps — semi-implicit Euler is stable for these
     * stiffnesses only with steps this short. Returns whether it is still moving.
     */
    fun step(seconds: Float): Boolean {
        var left = min(seconds, MAX_STEP)
        while (left > 0f) {
            val dt = min(left, SUB_STEP)
            val force = stiffness * (target - value) - damping * velocity
            velocity += force * dt
            value += velocity * dt
            left -= dt
        }
        if (settled) {
            value = target
            velocity = 0f
            return false
        }
        return true
    }

    private companion object {
        const val SUB_STEP = 1f / 480f
        const val MAX_STEP = 1f / 20f
        const val EPSILON = 0.001f
    }
}
