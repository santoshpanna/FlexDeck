package dev.flexdeck.client

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class JsonPointerTest {

    @Test
    fun `the empty pointer has no tokens`() {
        assertEquals(emptyList(), JsonPointer.tokens(""))
    }

    @Test
    fun `a leading slash means one empty token`() {
        // RFC 6901 distinguishes "" (the whole document) from "/" (the member
        // named ""). Collapsing them would make an empty property name
        // unaddressable.
        assertEquals(listOf(""), JsonPointer.tokens("/"))
    }

    @Test
    fun `ordinary paths split on slashes`() {
        assertEquals(listOf("profiles", "0", "widgets"), JsonPointer.tokens("/profiles/0/widgets"))
    }

    @Test
    fun `escaped tokens are decoded`() {
        assertEquals(listOf("a/b"), JsonPointer.tokens("/a~1b"))
        assertEquals(listOf("a~b"), JsonPointer.tokens("/a~0b"))
    }

    @Test
    fun `the escape sequences decode in the order the spec requires`() {
        // The one that catches people out: `~01` is `~1`, not `/`. Decoding `~0`
        // first would produce `~` followed by `1`... and then a second pass would
        // turn it into a slash. This is a silent wrong-field bug, so it is worth
        // its own test.
        assertEquals(listOf("~1"), JsonPointer.tokens("/~01"))
    }

    @Test
    fun `formatting round trips`() {
        val awkward = listOf("a/b", "a~b", "~1", "", "plain")
        assertEquals(awkward, JsonPointer.tokens(JsonPointer.format(awkward)))
    }

    @Test
    fun `a pointer that does not start with a slash is rejected`() {
        assertFailsWith<IllegalArgumentException> { JsonPointer.tokens("profiles/0") }
    }
}
