package dev.flexdeck.client

import kotlin.math.abs
import kotlin.math.sign

/**
 * Swiping between a deck's pages, as arithmetic.
 *
 * A finger that lands on a key is a press until it has moved clearly sideways;
 * then it becomes a swipe, the press is called off, and the page follows the
 * finger. Past the first or last page it follows reluctantly, so the edge is
 * felt rather than hit. On lift, a long enough drag or a quick enough flick
 * turns the page; anything less springs back.
 *
 * Pure so the thresholds are tested here and `DeckView` only draws.
 */
object Pager {
    /** How much of a drag past the first or last page shows. */
    const val EDGE_RESISTANCE = 0.3f

    /** The share of the width a slow drag must cover to turn the page. */
    const val TURN_FRACTION = 0.18f

    /** A flick this fast (px/s) turns the page however short it was. */
    const val FLICK_VELOCITY = 900f

    /** How long a key's press is held back on a paged deck, in case it is a swipe. */
    const val PRESS_DELAY_MS = 150L

    /**
     * How far (dp) a finger that landed on a key must run sideways to swipe —
     * wider than the system's touch slop, so a shaky press stays a press.
     */
    const val KEY_SLOP_DP = 24f

    /**
     * Whether a finger that has moved (`dx`, `dy`) from where it landed is now
     * swiping. It must clear `slop` and run mostly sideways, so a press that
     * wobbles stays a press and a vertical scroll-ish motion is not taken.
     */
    fun claims(dx: Float, dy: Float, slop: Float): Boolean =
        abs(dx) > slop && abs(dx) > abs(dy) * 1.5f

    /** How far the page shows moved while dragged `dx` on page `index` of `count`. */
    fun offset(dx: Float, index: Int, count: Int): Float {
        val pastStart = index <= 0 && dx > 0f
        val pastEnd = index >= count - 1 && dx < 0f
        return if (pastStart || pastEnd) dx * EDGE_RESISTANCE else dx
    }

    /**
     * The page to settle on when the finger lifts after a drag of `dx` at
     * `velocity` (px/s, positive rightwards) across a deck `width` wide.
     * Dragging leftwards moves on to the next page.
     */
    fun settle(dx: Float, velocity: Float, width: Float, index: Int, count: Int): Int {
        if (count <= 1) return 0
        val far = width > 0f && abs(dx) > width * TURN_FRACTION
        // A flick counts only in the direction the finger actually went.
        val flicked = abs(velocity) > FLICK_VELOCITY && sign(velocity) == sign(dx)
        if (!far && !flicked) return index.coerceIn(0, count - 1)
        val step = if (dx < 0f) 1 else -1
        return (index + step).coerceIn(0, count - 1)
    }
}
