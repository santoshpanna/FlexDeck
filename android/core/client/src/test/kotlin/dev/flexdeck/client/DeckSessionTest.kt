package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DeckSessionTest {

    /**
     * A welcome whose *document* carries the same revision it declares.
     *
     * Keeping those in step matters: a fixture where they disagree makes every
     * assertion about the document meaningless, which is exactly the kind of
     * thing a test is supposed to catch rather than contain.
     */
    private fun welcome(rev: Long) =
        ServerMessage.Welcome(
            rev = rev,
            profileId = "tab1",
            config =
                """
                {"schema":1,"rev":$rev,"profiles":[{"id":"tab1","pages":[{"id":"home","widgets":[]}]}]}
                """.trimIndent().toObject(),
        )

    private fun patch(rev: Long, operations: String) =
        ServerMessage.Patch(rev = rev, patch = Json.parseToJsonElement(operations) as JsonArray)

    private fun String.toObject(): JsonObject =
        Json.parseToJsonElement(this) as JsonObject

    private fun DeckSession.revOfDocument(): Long =
        ((config?.get("rev") as? JsonPrimitive)?.content?.toLongOrNull() ?: -1)

    @Test
    fun `a welcome establishes the document`() {
        val session = DeckSession()

        session.apply(welcome(1))

        assertEquals(1L, session.rev)
        assertTrue(session.hasConfig)
        assertFalse(session.needsResync)
    }

    @Test
    fun `a patch advances the document`() {
        val session = DeckSession()
        session.apply(welcome(1))

        session.apply(patch(2, """[{"op":"replace","path":"/rev","value":2}]"""))

        assertEquals(2L, session.rev)
        assertEquals(2L, session.revOfDocument(), "the patch must have been applied, not just recorded")
    }

    @Test
    fun `a patch older than the document is ignored`() {
        // An undo broadcasts the revision it moved *to*. Applying it would walk
        // the client backwards while the agent believes it is current.
        val session = DeckSession()
        session.apply(welcome(5))
        session.apply(patch(6, """[{"op":"replace","path":"/rev","value":6}]"""))

        session.apply(patch(3, """[{"op":"remove","path":"/profiles"}]"""))

        assertEquals(6L, session.rev, "the older patch must not have been applied")
        assertTrue(session.hasConfig)
        assertFalse(session.needsResync, "ignoring a stale patch is normal, not a failure")
    }

    @Test
    fun `re-applying the current revision is harmless`() {
        val session = DeckSession()
        session.apply(welcome(2))

        session.apply(patch(2, """[{"op":"replace","path":"/rev","value":99}]"""))

        assertEquals(2L, session.rev)
        assertEquals(2L, session.revOfDocument())
    }

    @Test
    fun `a patch that cannot be applied asks for a resync`() {
        // The important part is that it does NOT throw and does NOT half-apply.
        // A client that carried on would draw a document that never existed.
        val session = DeckSession()
        session.apply(welcome(1))

        session.apply(patch(2, """[{"op":"replace","path":"/nope","value":1}]"""))

        assertTrue(session.needsResync, "a failed patch must be visible")
        assertEquals(1L, session.rev, "and the revision must not have moved")
        assertEquals(1L, session.revOfDocument())
    }

    @Test
    fun `a patch before any document asks for a resync`() {
        val session = DeckSession()

        session.apply(patch(4, """[{"op":"add","path":"/rev","value":4}]"""))

        assertTrue(session.needsResync)
        assertFalse(session.hasConfig)
    }

    @Test
    fun `a refusal from the agent asks for a resync`() {
        val session = DeckSession()

        session.apply(ServerMessage.Failure(code = "unauthorised", message = "revoked"))

        assertTrue(session.needsResync)
    }

    @Test
    fun `resyncing recovers, and a good welcome clears the flag`() {
        val session = DeckSession()
        session.apply(welcome(1))
        session.apply(patch(2, """[{"op":"replace","path":"/nope","value":1}]"""))
        assertTrue(session.needsResync)

        session.apply(welcome(9))

        assertFalse(session.needsResync, "a fresh document is exactly what was asked for")
        assertEquals(9L, session.rev)
    }

    @Test
    fun `a patch older than a re-synced document is still ignored`() {
        // The trap: after resyncing forwards, a queue of old patches must not
        // drag the client back.
        val session = DeckSession()
        session.apply(welcome(10))

        session.apply(patch(4, """[{"op":"remove","path":"/profiles"}]"""))
        session.apply(patch(5, """[{"op":"remove","path":"/profiles"}]"""))

        assertEquals(10L, session.rev)
        assertTrue(session.config?.containsKey("profiles") == true)
    }

    @Test
    fun `reset forgets everything`() {
        val session = DeckSession()
        session.apply(welcome(3))

        session.reset()

        assertNull(session.config)
        assertEquals(0L, session.rev)
        assertFalse(session.needsResync)
    }

    @Test
    fun `widget state does not disturb the document`() {
        // A state event redraws a button. Treating it as a configuration change
        // would be a category error, and one that silently corrupts the view.
        val session = DeckSession()
        session.apply(welcome(1))

        session.apply(ServerMessage.State(widgetId = "mute", stateId = "on"))

        assertEquals(1L, session.rev)
        assertEquals(1L, session.revOfDocument())
        assertFalse(session.needsResync)
    }

    @Test
    fun `the latest state of a widget wins, since each frame carries its whole look`() {
        val session = DeckSession()
        session.apply(welcome(1))

        session.apply(ServerMessage.State("rec", "recording", "REC 00:00:01"))
        assertEquals(LiveWidget("recording", "REC 00:00:01"), session.liveWidgets["rec"])

        session.apply(ServerMessage.State("rec", "idle", "REC 00:00:00"))
        assertEquals("idle", session.liveWidgets["rec"]?.stateId)

        // A plain widget's press answer: no look, no label — its own props.
        session.apply(ServerMessage.State("plain"))
        assertEquals(LiveWidget("", ""), session.liveWidgets["plain"])
    }

    @Test
    fun `live state is forgotten on reset, since the agent resends it`() {
        val session = DeckSession()
        session.apply(ServerMessage.State("rec", "on", ""))
        session.reset()
        assertTrue(session.liveWidgets.isEmpty())
    }
}
