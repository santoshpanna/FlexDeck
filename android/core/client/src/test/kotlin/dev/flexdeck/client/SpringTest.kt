package dev.flexdeck.client

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SpringTest {
    @Test
    fun `it settles on its target`() {
        val spring = Spring(0f)
        spring.target = 1f
        var frames = 0
        while (spring.step(1f / 60f)) frames++
        assertEquals(1f, spring.value)
        assertTrue(spring.settled)
        assertTrue(frames in 3..60, "settled after $frames frames")
    }

    @Test
    fun `a stiff key spring overshoots a little, never a lot`() {
        val spring = Spring(0f)
        spring.target = 1f
        var peak = 0f
        repeat(120) {
            spring.step(1f / 60f)
            peak = maxOf(peak, spring.value)
        }
        assertTrue(peak > 1f, "no overshoot at all: $peak")
        assertTrue(peak < 1.15f, "overshoot too large: $peak")
    }

    @Test
    fun `a stalled frame does not fling it`() {
        val spring = Spring(0f)
        spring.target = 1f
        spring.step(5f)
        assertTrue(spring.value in -0.5f..1.5f)
    }

    @Test
    fun `snap jumps without motion`() {
        val spring = Spring(0f)
        spring.snap(3f)
        assertFalse(spring.step(1f / 60f))
        assertEquals(3f, spring.value)
    }
}
