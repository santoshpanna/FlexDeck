package dev.flexdeck.client

import java.util.Locale
import java.util.TimeZone
import kotlin.test.Test
import kotlin.test.assertEquals

class ClockTest {

    /** 2026-10-11 21:05:09 UTC, a Sunday. */
    private val now = 1_791_752_709_000L
    private val utc = TimeZone.getTimeZone("UTC")

    @Test
    fun `strftime formats become patterns with their text quoted`() {
        assertEquals("HH':'mm", Clock.pattern("%H:%M"))
        assertEquals("h':'mm' 'a", Clock.pattern("%-I:%M %p"))
        assertEquals("'at %'HH", Clock.pattern("at %%%H"))
        assertEquals("'it''s '", Clock.pattern("it's "), "a quote in the text is doubled")
        assertEquals("'%q'", Clock.pattern("%q"), "an unknown code stays as text")
    }

    @Test
    fun `a marquee keeps moving at today's time`() {
        val at = Clock.scrolled(now, 36.0, 500.0)
        assertEquals(1.2, Clock.scrolled(now + 33, 36.0, 500.0) - at, 0.05)
        assertEquals(360.0, Clock.scrolled(10_000, 36.0, 500.0), 1e-9)
        assertEquals(220.0, Clock.scrolled(20_000, 36.0, 500.0), 1e-9, "comes round again")
        assertEquals(0.0, Clock.scrolled(now, 36.0, 0.0))
    }

    @Test
    fun `a clock reads the time in its zone`() {
        assertEquals("21:05", Clock.text(now, "", utc, seconds = false, Locale.US))
        assertEquals("21:05:09", Clock.text(now, "", utc, seconds = true, Locale.US))
        assertEquals("9:05 PM", Clock.text(now, "%-I:%M %p", utc, seconds = false, Locale.US))
        assertEquals("Mon 06:05", Clock.text(now, "%a %H:%M", Clock.zone("Asia/Tokyo"), seconds = false, Locale.US))
        assertEquals("Sun 11 Oct", Clock.text(now, "%a %-d %b", utc, seconds = false, Locale.US))
    }

    @Test
    fun `an unknown zone is the tablet's own, not GMT`() {
        assertEquals(TimeZone.getDefault().id, Clock.zone("Mars/Olympus").id)
        assertEquals("GMT", Clock.zone("GMT").id)
    }

    @Test
    fun `the hands point where a clock's would`() {
        val (hour, minute, second) = Clock.hands(now, utc)
        // 9:05:09 — the hour hand just past the 9.
        assertEquals(272.6f, hour, 0.1f)
        assertEquals(30.9f, minute, 0.1f)
        assertEquals(54f, second, 0.1f)
    }
}
