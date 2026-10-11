package dev.flexdeck.client

/**
 * A JSON Pointer, as defined by RFC 6901.
 *
 * The escaping is the part worth getting exactly right. `~1` means `/` and `~0`
 * means `~`, and the spec requires them to be decoded **in that order** — so the
 * token `~01` decodes to `~1`, not to `/`.
 *
 * Decoding in the wrong order is a quiet failure, not a loud one: a widget id
 * containing a slash would patch a field that does not exist, and the client
 * would go on rendering whatever it already had. That is why this has its own
 * tests rather than being inlined into the patch code.
 */
object JsonPointer {

    /** Split a pointer into its decoded tokens. `""` is the whole document. */
    fun tokens(pointer: String): List<String> {
        if (pointer.isEmpty()) {
            return emptyList()
        }

        require(pointer.startsWith("/")) {
            "a JSON Pointer must be empty or start with '/', but was '$pointer'"
        }

        return pointer.substring(1).split('/').map(::unescape)
    }

    /**
     * Join tokens back into a pointer.
     *
     * Escaping happens in the reverse order of unescaping: `~` first, then `/`.
     * Doing it the other way would turn a literal `~1` into `/`.
     */
    fun format(tokens: List<String>): String =
        tokens.joinToString(separator = "/", prefix = "/") { escape(it) }

    private fun unescape(token: String): String =
        token.replace("~1", "/").replace("~0", "~")

    private fun escape(token: String): String =
        token.replace("~", "~0").replace("/", "~1")
}
