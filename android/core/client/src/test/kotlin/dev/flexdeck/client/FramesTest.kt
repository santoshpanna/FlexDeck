package dev.flexdeck.client

import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * These frames are written in the exact shape the agent emits, including the
 * parts it *omits*. The omissions are the point: proto3 JSON drops default
 * values, so a generated parser would hide them and a hand-written one has to
 * decide what they mean.
 */
class FramesTest {

    @Test
    fun `a welcome carries the revision and the configuration`() {
        val frame = """
            {"welcome":{"rev":7,"profileId":"tab1","config":{"schema":1,"profiles":[]}}}
        """.trimIndent()

        val message = Frames.parseServerMessage(frame) as ServerMessage.Welcome

        assertEquals(7L, message.rev)
        assertEquals("tab1", message.profileId)
        assertTrue(message.config != null, "the configuration should be present")
    }

    @Test
    fun `an omitted profileId is an empty string, not a failure`() {
        // The agent sends "" for a profile it has not chosen, and proto3 JSON
        // omits it entirely.
        val frame = """{"welcome":{"rev":1,"config":{"schema":1}}}"""

        val message = Frames.parseServerMessage(frame) as ServerMessage.Welcome

        assertEquals("", message.profileId)
        assertEquals(1L, message.rev)
    }

    @Test
    fun `a welcome with no configuration at all parses`() {
        // Means the agent has nothing committed; the client should fall back to
        // whatever it cached rather than treating this as corruption.
        val message = Frames.parseServerMessage("""{"welcome":{"rev":0}}""") as ServerMessage.Welcome

        assertNull(message.config)
        assertEquals(0L, message.rev, "an omitted rev is zero, per the mapping")
    }

    @Test
    fun `a patch is decoded from the string it is wrapped in`() {
        // The patch travels double-encoded: a JSON string inside a JSON message.
        // Missing that second parse yields a string, not operations, and the
        // client would silently ignore every update.
        val frame = """
            {"patch":{"rev":8,"patch":"[{\"op\":\"replace\",\"path\":\"/rev\",\"value\":8}]"}}
        """.trimIndent()

        val message = Frames.parseServerMessage(frame) as ServerMessage.Patch

        assertEquals(8L, message.rev)
        assertEquals(1, message.patch.size)
        val operation = message.patch[0].let { it as kotlinx.serialization.json.JsonObject }
        assertEquals("replace", (operation["op"] as JsonPrimitive).content)
    }

    @Test
    fun `a patch whose payload is not an array is refused`() {
        // Better to fail than to apply nothing and look healthy.
        val frame = """{"patch":{"rev":8,"patch":"\"not an array\""}}"""

        assertFailsWith<FrameException> { Frames.parseServerMessage(frame) }
    }

    @Test
    fun `a patch is actually applicable to a document`() {
        // The two halves joined up: parse a real frame, then apply it.
        val frame = """
            {"patch":{"rev":2,"patch":"[{\"op\":\"replace\",\"path\":\"/rev\",\"value\":2}]"}}
        """.trimIndent()
        val message = Frames.parseServerMessage(frame) as ServerMessage.Patch

        val document = kotlinx.serialization.json.Json.parseToJsonElement("""{"rev":1}""")
        val updated = JsonPatch.apply(document, message.patch)

        assertEquals("""{"rev":2}""", updated.toString())
    }

    @Test
    fun `a state event parses`() {
        val frame = """{"state":{"widgetId":"mute","stateId":"on","ok":true}}"""

        val message = Frames.parseServerMessage(frame) as ServerMessage.State

        assertEquals("mute", message.widgetId)
        assertEquals("on", message.stateId)
    }

    @Test
    fun `an error frame parses`() {
        val frame = """{"error":{"code":"unauthorised","message":"unknown or revoked device token"}}"""

        val message = Frames.parseServerMessage(frame) as ServerMessage.Failure

        assertEquals("unauthorised", message.code)
        assertTrue(message.message.contains("token"))
    }

    @Test
    fun `an unrecognised frame is refused`() {
        assertFailsWith<FrameException> { Frames.parseServerMessage("""{"nonsense":{}}""") }
        assertFailsWith<FrameException> { Frames.parseServerMessage("not json") }
    }

    @Test
    fun `an omitted ok means a refused pairing`() {
        // proto3 JSON omits `false`, so a refusal has no `ok` key at all. Testing
        // for `ok == false` would never match; the client must treat absence as
        // false.
        assertTrue(Frames.parsePairingGranted("""{"token":"abc"}""") == false)
        assertTrue(Frames.parsePairingGranted("""{"ok":true,"token":"abc"}"""))
    }

    @Test
    fun `a live state carries its label, and an omitted ok is success`() {
        val frame = """{"state":{"widgetId":"rec","stateId":"on","label":"REC 00:01:05"}}"""
        val message = Frames.parseServerMessage(frame) as ServerMessage.State

        assertEquals("rec", message.widgetId)
        assertEquals("on", message.stateId)
        assertEquals("REC 00:01:05", message.label)
        assertEquals(true, message.ok)
    }

    @Test
    fun `a failed press says so`() {
        val frame = """{"state":{"widgetId":"x","ok":false,"error":"OBS is not connected"}}"""
        val message = Frames.parseServerMessage(frame) as ServerMessage.State

        assertEquals(false, message.ok)
        assertEquals("OBS is not connected", message.error)
    }

    @Test
    fun `a face's element values ride on the state, absent fields at their defaults`() {
        val frame = """{"state":{"widgetId":"cpu","elements":[{"text":"CPU"},{"index":1,"number":41.5,"color":"#ef4444","pulse":true}],"background":"#7f1d1d","pulse":true,"backgroundImage":"img:ab"}}"""
        val message = Frames.parseServerMessage(frame) as ServerMessage.State

        assertEquals(ElementLive(index = 0, text = "CPU"), message.elements[0])
        assertEquals(ElementLive(index = 1, number = 41.5, color = "#ef4444", pulse = true), message.elements[1])
        assertEquals("#7f1d1d", message.background)
        assertTrue(message.pulse)
        assertEquals("img:ab", message.backgroundImage)
    }

    @Test
    fun `a series keeps its finite points and a page names where to go`() {
        val series = Frames.parseServerMessage("""{"series":{"variable":"cpu","points":[1,2.5,"NaN"],"append":true}}""") as ServerMessage.Series
        assertEquals(ServerMessage.Series("cpu", listOf(1.0, 2.5), append = true), series)

        val page = Frames.parseServerMessage("""{"page":{"step":-1}}""") as ServerMessage.Page
        assertEquals(ServerMessage.Page(pageId = "", step = -1), page)
    }

    @Test
    fun `a session keeps each series whole or appended, within its cap`() {
        val session = DeckSession()
        session.apply(ServerMessage.Series("cpu", listOf(1.0, 2.0), append = false))
        session.apply(ServerMessage.Series("cpu", listOf(3.0), append = true))
        assertEquals(listOf(1.0, 2.0, 3.0), session.series["cpu"])
        session.apply(ServerMessage.Series("cpu", (1..700).map { it.toDouble() }, append = false))
        assertEquals(600, session.series["cpu"]!!.size)
        assertEquals(700.0, session.series["cpu"]!!.last())
    }
}
