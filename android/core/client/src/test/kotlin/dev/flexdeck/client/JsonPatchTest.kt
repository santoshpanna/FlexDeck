package dev.flexdeck.client

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.int
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class JsonPatchTest {

    private fun json(text: String): JsonElement = Json.parseToJsonElement(text)

    private fun apply(document: String, patch: String): String =
        JsonPatch.apply(json(document), json(patch) as JsonArray).toString()

    /** Walk a document by object keys and array indices. */
    private fun JsonElement.at(vararg path: String): JsonElement {
        var current = this
        for (token in path) {
            current = when (current) {
                is JsonObject -> current[token] ?: error("no '$token' in $current")
                is JsonArray -> current[token.toIntOrNull() ?: error("'$token' is not an index")]
                else -> error("cannot index into $current")
            }
        }
        return current
    }

    @Test
    fun `adding a member to an object`() {
        assertEquals(
            """{"a":1,"b":2}""",
            apply("""{"a":1}""", """[{"op":"add","path":"/b","value":2}]"""),
        )
    }

    @Test
    fun `adding replaces an existing member`() {
        // RFC 6902 says `add` on an existing object member replaces it. The name
        // is misleading and this is the behaviour the agent's Rust side has.
        assertEquals(
            """{"a":9}""",
            apply("""{"a":1}""", """[{"op":"add","path":"/a","value":9}]"""),
        )
    }

    @Test
    fun `adding into an array inserts rather than replaces`() {
        assertEquals(
            """{"a":[1,9,2]}""",
            apply("""{"a":[1,2]}""", """[{"op":"add","path":"/a/1","value":9}]"""),
        )
    }

    @Test
    fun `a dash appends to an array`() {
        assertEquals(
            """{"a":[1,2,9]}""",
            apply("""{"a":[1,2]}""", """[{"op":"add","path":"/a/-","value":9}]"""),
        )
    }

    @Test
    fun `removing a member`() {
        assertEquals("""{"a":1}""", apply("""{"a":1,"b":2}""", """[{"op":"remove","path":"/b"}]"""))
    }

    @Test
    fun `removing an array element shifts the rest`() {
        assertEquals("""{"a":[1,3]}""", apply("""{"a":[1,2,3]}""", """[{"op":"remove","path":"/a/1"}]"""))
    }

    @Test
    fun `replacing a member`() {
        assertEquals("""{"a":9}""", apply("""{"a":1}""", """[{"op":"replace","path":"/a","value":9}]"""))
    }

    @Test
    fun `replacing something that is not there fails loudly`() {
        // The alternative — creating it — would make a client disagree with the
        // agent about what the document contains and never notice.
        assertFailsWith<JsonPatchException> {
            apply("""{"a":1}""", """[{"op":"replace","path":"/b","value":9}]""")
        }
    }

    @Test
    fun `removing something that is not there fails loudly`() {
        assertFailsWith<JsonPatchException> {
            apply("""{"a":1}""", """[{"op":"remove","path":"/b"}]""")
        }
    }

    @Test
    fun `an index past the end is refused`() {
        assertFailsWith<JsonPatchException> {
            apply("""{"a":[1]}""", """[{"op":"replace","path":"/a/5","value":9}]""")
        }
        // `add` may address one past the end (that is an append), but no further.
        assertFailsWith<JsonPatchException> {
            apply("""{"a":[1]}""", """[{"op":"add","path":"/a/5","value":9}]""")
        }
        assertEquals("""{"a":[1,9]}""", apply("""{"a":[1]}""", """[{"op":"add","path":"/a/1","value":9}]"""))
    }

    @Test
    fun `a failing test operation stops the patch`() {
        assertFailsWith<JsonPatchException> {
            apply("""{"a":1}""", """[{"op":"test","path":"/a","value":2}]""")
        }
    }

    @Test
    fun `a passing test operation changes nothing`() {
        assertEquals("""{"a":1}""", apply("""{"a":1}""", """[{"op":"test","path":"/a","value":1}]"""))
    }

    @Test
    fun `operations apply in order`() {
        // Each operation sees the result of the last, which is the whole reason
        // a patch can be a sequence rather than a set.
        assertEquals(
            """{"a":2,"b":3}""",
            apply("""{"a":1}""", """[{"op":"add","path":"/b","value":3},{"op":"replace","path":"/a","value":2}]"""),
        )
    }

    @Test
    fun `an unknown operation is refused rather than skipped`() {
        // Skipping it would leave the client holding a document the agent does
        // not think it sent, and nothing would ever say so.
        assertFailsWith<JsonPatchException> {
            apply("""{"a":1}""", """[{"op":"frobnicate","path":"/a"}]""")
        }
    }

    @Test
    fun `move and copy are supported`() {
        assertEquals("""{"b":1}""", apply("""{"a":1}""", """[{"op":"move","from":"/a","path":"/b"}]"""))
        assertEquals("""{"a":1,"b":1}""", apply("""{"a":1}""", """[{"op":"copy","from":"/a","path":"/b"}]"""))
    }

    @Test
    fun `a path containing a slash survives the round trip`() {
        // Widget ids come from user data, so a slash in one is not hypothetical.
        assertEquals(
            """{"a/b":2}""",
            apply("""{"a/b":1}""", """[{"op":"replace","path":"/a~1b","value":2}]"""),
        )
    }

    @Test
    fun `a realistic agent patch reconstructs the next revision`() {
        // The shape the agent actually produces: members replaced deep inside the
        // configuration, addressed by array indices.
        val before = """
            {"schema":1,"rev":1,"profiles":[{"id":"tab1","pages":[{"id":"home","widgets":[
              {"id":"mute","col":0,"row":0,"w":1,"h":1,"props":{"label":"Mute"}}
            ]}]}]}
        """.trimIndent()

        val patch = """
            [{"op":"replace","path":"/profiles/0/pages/0/widgets/0/props/label","value":"Silence"},
             {"op":"replace","path":"/rev","value":2}]
        """.trimIndent()

        val after = JsonPatch.apply(json(before), json(patch) as JsonArray)

        assertEquals(2, (after.at("rev") as JsonPrimitive).int)
        assertEquals(
            "Silence",
            (after.at("profiles", "0", "pages", "0", "widgets", "0", "props", "label") as JsonPrimitive).content,
        )
        // And nothing else moved, which is the half of a patch that is easy to
        // get wrong and never notice.
        assertEquals("mute", (after.at("profiles", "0", "pages", "0", "widgets", "0", "id") as JsonPrimitive).content)
    }
}
