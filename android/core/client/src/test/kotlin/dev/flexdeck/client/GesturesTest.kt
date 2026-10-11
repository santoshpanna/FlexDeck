package dev.flexdeck.client

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class GesturesTest {
    private val tapOnly = setOf(Gesture.TAP)
    private val withLong = setOf(Gesture.TAP, Gesture.LONG_PRESS)
    private val withDouble = setOf(Gesture.TAP, Gesture.DOUBLE_TAP)
    private val hold = setOf(Gesture.HOLD_START, Gesture.HOLD_END)

    @Test
    fun `a plain key fires the moment a finger lands`() {
        val recognizer = GestureRecognizer()
        assertEquals(listOf(Fired("k", Gesture.TAP)), recognizer.down("k", tapOnly, 0))
        assertEquals(emptyList(), recognizer.up("k", tapOnly, 50))
        assertNull(recognizer.nextDeadline())
    }

    @Test
    fun `a key with no flows at all still taps`() {
        assertEquals(listOf(Fired("k", Gesture.TAP)), GestureRecognizer().down("k", emptySet(), 0))
    }

    @Test
    fun `a hold reports down and up and nothing else`() {
        val recognizer = GestureRecognizer()
        assertEquals(listOf(Fired("k", Gesture.HOLD_START)), recognizer.down("k", hold, 0))
        assertEquals(emptyList(), recognizer.tick(2_000))
        assertEquals(listOf(Fired("k", Gesture.HOLD_END)), recognizer.up("k", hold, 2_000))
    }

    @Test
    fun `a cancelled hold still ends, so a mic is never left open`() {
        val recognizer = GestureRecognizer()
        recognizer.down("k", hold, 0)
        assertEquals(listOf(Fired("k", Gesture.HOLD_END)), recognizer.cancel("k", hold))
    }

    @Test
    fun `a long press fires once, and the release after it does not tap`() {
        val recognizer = GestureRecognizer(longPressMs = 500)
        assertEquals(emptyList(), recognizer.down("k", withLong, 0))
        assertEquals(500L, recognizer.nextDeadline())
        assertEquals(emptyList(), recognizer.tick(499))
        assertEquals(listOf(Fired("k", Gesture.LONG_PRESS)), recognizer.tick(500))
        assertEquals(emptyList(), recognizer.tick(900))
        assertEquals(emptyList(), recognizer.up("k", withLong, 900))
    }

    @Test
    fun `a short press on a long-press key taps on release`() {
        val recognizer = GestureRecognizer(longPressMs = 500)
        recognizer.down("k", withLong, 0)
        assertEquals(listOf(Fired("k", Gesture.TAP)), recognizer.up("k", withLong, 120))
        assertNull(recognizer.nextDeadline())
    }

    @Test
    fun `two quick taps make a double tap and no single tap`() {
        val recognizer = GestureRecognizer(doubleTapMs = 280)
        recognizer.down("k", withDouble, 0)
        assertEquals(emptyList(), recognizer.up("k", withDouble, 60))
        recognizer.down("k", withDouble, 150)
        assertEquals(listOf(Fired("k", Gesture.DOUBLE_TAP)), recognizer.up("k", withDouble, 200))
        assertEquals(emptyList(), recognizer.tick(1_000))
    }

    @Test
    fun `a lone tap on a double-tap key fires once the window closes`() {
        val recognizer = GestureRecognizer(doubleTapMs = 280)
        recognizer.down("k", withDouble, 0)
        recognizer.up("k", withDouble, 60)
        val deadline = recognizer.nextDeadline()
        assertTrue(deadline != null && deadline > 340)
        assertEquals(emptyList(), recognizer.tick(300))
        assertEquals(listOf(Fired("k", Gesture.TAP)), recognizer.tick(deadline!!))
    }

    @Test
    fun `keys are recognised independently`() {
        val recognizer = GestureRecognizer(longPressMs = 500)
        recognizer.down("a", withLong, 0)
        assertEquals(listOf(Fired("b", Gesture.TAP)), recognizer.down("b", tapOnly, 100))
        assertEquals(listOf(Fired("a", Gesture.LONG_PRESS)), recognizer.tick(500))
    }
}
