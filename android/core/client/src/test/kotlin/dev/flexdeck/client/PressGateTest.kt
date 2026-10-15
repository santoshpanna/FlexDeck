package dev.flexdeck.client

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull

class PressGateTest {
    private val slop = 24f

    private fun landed(at: Long = 1000) = PressGate(delayMs = 150).also { it.down(at) }

    @Test
    fun `a clear sideways run before the moment passes is a swipe`() {
        val gate = landed()
        assertEquals(PressGate.Outcome.DISCARD, gate.move(dx = -40f, dy = 4f, slop, now = 1100))
        assertFalse(gate.pending)
        assertEquals(PressGate.Outcome.NONE, gate.up())
    }

    @Test
    fun `a quick tap that wobbled still presses`() {
        val gate = landed()
        assertEquals(PressGate.Outcome.NONE, gate.move(dx = 12f, dy = -6f, slop, now = 1050))
        assertEquals(PressGate.Outcome.COMMIT, gate.up())
    }

    @Test
    fun `a shaky finger held past the moment presses`() {
        val gate = landed()
        assertEquals(PressGate.Outcome.NONE, gate.move(dx = -18f, dy = 3f, slop, now = 1100))
        assertEquals(PressGate.Outcome.COMMIT, gate.tick(1150))
    }

    @Test
    fun `once a press, never a swipe`() {
        val gate = landed()
        gate.tick(1150)
        assertEquals(PressGate.Outcome.NONE, gate.move(dx = -300f, dy = 0f, slop, now = 1200))
        assertEquals(PressGate.Outcome.NONE, gate.up())
    }

    @Test
    fun `a late move after the moment passed is a press, not a swipe`() {
        val gate = landed()
        assertEquals(PressGate.Outcome.COMMIT, gate.move(dx = -300f, dy = 0f, slop, now = 1160))
        assertFalse(gate.pending)
    }

    @Test
    fun `the moment ends exactly at the delay`() {
        val gate = landed()
        assertEquals(1150L, gate.nextDeadline())
        assertEquals(PressGate.Outcome.NONE, gate.tick(1149))
        assertEquals(PressGate.Outcome.COMMIT, gate.tick(1150))
        assertNull(gate.nextDeadline())
    }

    @Test
    fun `a second finger makes it a press`() {
        assertEquals(PressGate.Outcome.COMMIT, landed().otherFinger())
    }

    @Test
    fun `a cancelled gesture sends nothing`() {
        val gate = landed()
        assertEquals(PressGate.Outcome.DISCARD, gate.cancel())
        assertEquals(PressGate.Outcome.NONE, gate.tick(5000))
    }

    @Test
    fun `it settles once`() {
        val gate = landed()
        assertEquals(PressGate.Outcome.COMMIT, gate.up())
        assertEquals(PressGate.Outcome.NONE, gate.up())
        assertEquals(PressGate.Outcome.NONE, gate.otherFinger())
        assertEquals(PressGate.Outcome.NONE, gate.tick(5000))
        assertEquals(PressGate.Outcome.NONE, gate.cancel())
        assertFalse(gate.pending)
    }
}
