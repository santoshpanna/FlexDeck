package dev.flexdeck.client

/** The gesture names the agent understands, as they travel on the wire. */
object Gesture {
    const val TAP = "GESTURE_SHORT_PRESS"
    const val DOUBLE_TAP = "GESTURE_DOUBLE_TAP"
    const val LONG_PRESS = "GESTURE_LONG_PRESS"
    const val HOLD_START = "GESTURE_HOLD_START"
    const val HOLD_END = "GESTURE_HOLD_END"
}

/** A gesture recognised on a widget. */
data class Fired(val widgetId: String, val gesture: String)

/**
 * Turns fingers going down and up into the gestures a key has flows for.
 *
 * The rule that keeps the deck feeling instant: a key waits only when it has to.
 *  - A plain key fires its tap the moment a finger lands.
 *  - A key with a hold reports down and up, and nothing else.
 *  - A key with a long press fires its tap on release instead — or the long
 *    press, once the finger has stayed down long enough.
 *  - A key with a double tap holds its first tap back for a moment, in case a
 *    second one follows.
 *
 * Pure: time comes in as `now`, and timers are the caller's — it asks
 * [nextDeadline] when to call [tick].
 */
class GestureRecognizer(
    private val longPressMs: Long = 500,
    private val doubleTapMs: Long = 280,
) {
    private class Press(val gestures: Set<String>, val at: Long) {
        var longFired = false
    }

    private val pressed = HashMap<String, Press>()
    private val pendingTap = HashMap<String, Long>()

    fun down(widgetId: String, gestures: Set<String>, now: Long): List<Fired> {
        if (holds(gestures)) return listOf(Fired(widgetId, Gesture.HOLD_START))
        if (!waits(gestures)) return listOf(Fired(widgetId, Gesture.TAP))
        pressed[widgetId] = Press(gestures, now)
        return emptyList()
    }

    fun up(widgetId: String, gestures: Set<String>, now: Long): List<Fired> {
        if (holds(gestures)) return listOf(Fired(widgetId, Gesture.HOLD_END))
        val press = pressed.remove(widgetId) ?: return emptyList()
        if (press.longFired) return emptyList()
        if (Gesture.DOUBLE_TAP !in press.gestures) return listOf(Fired(widgetId, Gesture.TAP))
        val first = pendingTap.remove(widgetId)
        if (first != null && now - first <= doubleTapMs) return listOf(Fired(widgetId, Gesture.DOUBLE_TAP))
        pendingTap[widgetId] = now
        return emptyList()
    }

    /** The finger was taken away by the system: nothing it began fires, except a hold's end. */
    fun cancel(widgetId: String, gestures: Set<String>): List<Fired> {
        pressed.remove(widgetId)
        return if (holds(gestures)) listOf(Fired(widgetId, Gesture.HOLD_END)) else emptyList()
    }

    /** Fire whatever has come due: long presses held long enough, first taps nobody followed. */
    fun tick(now: Long): List<Fired> {
        val fired = ArrayList<Fired>()
        for ((id, press) in pressed) {
            if (!press.longFired && Gesture.LONG_PRESS in press.gestures && now - press.at >= longPressMs) {
                press.longFired = true
                fired += Fired(id, Gesture.LONG_PRESS)
            }
        }
        val lapsed = pendingTap.filterValues { now - it > doubleTapMs }.keys
        for (id in lapsed) {
            pendingTap.remove(id)
            fired += Fired(id, Gesture.TAP)
        }
        return fired
    }

    /** When [tick] next has something to do, or `null` when nothing is waiting. */
    fun nextDeadline(): Long? {
        val longs = pressed.values.filter { !it.longFired && Gesture.LONG_PRESS in it.gestures }.map { it.at + longPressMs }
        val doubles = pendingTap.values.map { it + doubleTapMs + 1 }
        return (longs + doubles).minOrNull()
    }

    private fun holds(gestures: Set<String>) = Gesture.HOLD_START in gestures || Gesture.HOLD_END in gestures

    private fun waits(gestures: Set<String>) = Gesture.LONG_PRESS in gestures || Gesture.DOUBLE_TAP in gestures
}
