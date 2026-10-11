package dev.flexdeck.client

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Looking after a screen that is never off: a slow pixel shift against
 * burn-in, and a dim window at night. Read from the profile's `screen`.
 */
data class ScreenCare(
    val shift: Boolean = false,
    val dim: Boolean = false,
    /** Minutes after midnight. */
    val dimFrom: Int = 0,
    val dimTo: Int = 0,
    /** Brightness while dimmed, 1–100. */
    val dimLevel: Int = 10,
) {
    /** Whether `minute` (after midnight) falls in the dim window; it may span midnight. */
    fun dimmedAt(minute: Int): Boolean {
        if (!dim || dimFrom == dimTo) return false
        return if (dimFrom < dimTo) minute in dimFrom until dimTo else minute >= dimFrom || minute < dimTo
    }

    companion object {
        /** How long the deck stays in one place, in milliseconds. */
        const val SHIFT_MS = 120_000L

        /** A square walk around the centre: never more than one step from it. */
        private val PATTERN = listOf(0 to 0, 1 to 0, 1 to 1, 0 to 1, -1 to 1, -1 to 0, -1 to -1, 0 to -1, 1 to -1)

        /** Where the deck sits at `nowMs`, in steps of `step` pixels. */
        fun offset(nowMs: Long, step: Float): Pair<Float, Float> {
            val (x, y) = PATTERN[((nowMs / SHIFT_MS) % PATTERN.size).toInt()]
            return x * step to y * step
        }

        fun read(screen: JsonObject?): ScreenCare {
            fun text(key: String) = (screen?.get(key) as? JsonPrimitive)?.content
            fun number(key: String) = text(key)?.toIntOrNull() ?: 0
            return ScreenCare(
                shift = text("shift") == "true",
                dim = text("dim") == "true",
                dimFrom = number("dimFrom").coerceIn(0, 1439),
                dimTo = number("dimTo").coerceIn(0, 1439),
                dimLevel = number("dimLevel").takeIf { it > 0 }?.coerceIn(1, 100) ?: 10,
            )
        }
    }
}
