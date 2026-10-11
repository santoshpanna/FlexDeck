package dev.flexdeck.client

import kotlin.math.min
import kotlin.math.pow

/**
 * How long to wait before trying to reconnect.
 *
 * Exponential with a ceiling, and **jittered** — but jittered from the *upper
 * half* of the window, never from zero.
 *
 * The jitter itself is not decoration: several tablets that lost the same agent
 * would otherwise retry in perfect lockstep, and every one of them would hit the
 * agent at the same instant. But a uniform draw over the whole window can return
 * zero, and a zero-millisecond delay is an immediate retry — the exact opposite
 * of backing off, and the one outcome that turns an agent which is merely asleep
 * into a tablet with a hot CPU and a flat battery, because every attempt is a
 * fresh TLS handshake. Every wait here is a real wait.
 *
 * The delay is returned rather than slept, so the policy is a pure calculation
 * and can be tested without a clock.
 */
class Backoff(
    private val initialMillis: Long = 500,
    // A panel left on a wall should back off like one: two minutes between
    // attempts is still prompt for a control surface, and far cheaper than the
    // old thirty-second ceiling if the agent stays away for a long time.
    private val maxMillis: Long = 120_000,
    private val factor: Double = 2.0,
    /** Returns [0, 1). Injected so tests are deterministic. */
    private val random: () -> Double = Math::random,
) {
    private var attempt = 0

    /** The ceiling this attempt is capped at, before jitter. */
    fun capMillis(): Long {
        val growth = initialMillis.toDouble() * factor.pow(attempt.toDouble())
        // `toLong` on a huge double saturates, so the min() is what actually
        // bounds it — the cast cannot overflow into a negative.
        return min(maxMillis, if (growth.isFinite()) growth.toLong() else maxMillis)
    }

    /**
     * The delay to use for this attempt, then advance.
     *
     * Draws from `[cap / 2, cap]` rather than `[0, cap]`. The spread is what
     * matters — it is what stops a synchronised herd — and the upper half keeps
     * most of it while making a zero-millisecond retry impossible.
     */
    fun nextDelayMillis(): Long {
        val cap = capMillis()
        attempt += 1

        val floor = cap / 2
        return (floor + random() * (cap - floor)).toLong().coerceIn(floor, cap)
    }

    /** Back to the first delay, after a connection succeeds. */
    fun reset() {
        attempt = 0
    }

    /** How many attempts have been made since the last reset. */
    fun attempts(): Int = attempt
}
