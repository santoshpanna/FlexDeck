package dev.flexdeck.client

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class BackoffTest {

    @Test
    fun `the first wait is the initial one`() {
        assertEquals(500L, Backoff(random = { 1.0 }).capMillis())
    }

    @Test
    fun `the cap doubles each attempt`() {
        val backoff = Backoff(random = { 1.0 })

        backoff.nextDelayMillis() // consumed attempt 0
        assertEquals(1000L, backoff.capMillis())
        backoff.nextDelayMillis()
        assertEquals(2000L, backoff.capMillis())
        assertEquals(2, backoff.attempts())
    }

    @Test
    fun `the cap never exceeds the ceiling`() {
        // Left unbounded this would reach years of delay after a few hundred
        // attempts, and the agent would never be retried again.
        val backoff = Backoff(initialMillis = 1000, maxMillis = 30_000, random = { 1.0 })

        repeat(50) { backoff.nextDelayMillis() }

        assertEquals(30_000L, backoff.capMillis())
    }

    @Test
    fun `the delay is inside the cap and never zero`() {
        // With a fixed draw the exact value is predictable, which is the only
        // reason the jitter can be tested at all.
        val top = Backoff(random = { 1.0 })
        assertEquals(500L, top.nextDelayMillis(), "a full draw waits the whole cap")

        val bottom = Backoff(random = { 0.0 })
        assertEquals(250L, bottom.nextDelayMillis(), "an empty draw still waits half the cap")
    }

    @Test
    fun `a delay is never zero`() {
        // The regression this exists for: the old policy drew uniformly over
        // [0, cap], so zero was a legal delay — an immediate retry. On a tablet
        // whose agent has gone away that turns a backoff into a hot loop of TLS
        // handshakes, which is the least helpful thing a flat battery could find.
        val backoff = Backoff(random = { 0.0 })

        repeat(50) {
            val delay = backoff.nextDelayMillis()
            assertTrue(delay > 0, "a delay must be a real wait, but was $delay")
        }
    }

    @Test
    fun `many attempts all stay within their cap`() {
        // The property that matters when it is wrong: a jittered delay that
        // escaped its cap would make a tablet wait longer than it should, or
        // (worse, with a negative) not wait at all and hammer the agent.
        var state = 0.0
        val backoff = Backoff(
            initialMillis = 100,
            maxMillis = 1_000,
            random = {
                // A cheap deterministic sequence, so this stays reproducible.
                state = (state + 0.37) % 1.0
                state
            },
        )

        repeat(200) {
            val delay = backoff.nextDelayMillis()
            // The floor is half the smallest cap, and it is the property that
            // stops a lost agent turning into a hammering loop.
            assertTrue(delay >= 50, "a delay must stay above its floor, but was $delay")
            assertTrue(delay <= 1_000, "a delay must never exceed the ceiling, but was $delay")
        }
    }

    @Test
    fun `reset returns to the first wait`() {
        val backoff = Backoff(random = { 1.0 })
        repeat(5) { backoff.nextDelayMillis() }
        assertEquals(16_000L, backoff.capMillis())

        backoff.reset()

        assertEquals(0, backoff.attempts())
        assertEquals(500L, backoff.capMillis())
    }
}
