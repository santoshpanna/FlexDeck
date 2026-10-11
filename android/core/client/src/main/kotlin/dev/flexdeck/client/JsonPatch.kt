package dev.flexdeck.client

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * RFC 6902 JSON Patch.
 *
 * This is the **client half** of the agent's sync protocol: the agent sends a
 * patch, the client applies it and is then holding the next revision. The Rust
 * side (`flexdeck_server::sync::apply_patch`) is the reference; this has to agree
 * with it, which is the same parity discipline as the grid algorithm.
 *
 * Everything is immutable — documents are rebuilt rather than mutated, matching
 * `JsonElement`'s own shape and making a partially-applied patch impossible to
 * observe.
 */
class JsonPatchException(message: String) : Exception(message)

object JsonPatch {

    /** Apply `operations` (an array of patch operations) to `document`. */
    fun apply(document: JsonElement, operations: JsonArray): JsonElement =
        operations.fold(document) { current, operation ->
            applyOne(current, operation)
        }

    private fun applyOne(document: JsonElement, operation: JsonElement): JsonElement {
        val fields = operation as? JsonObject
            ?: throw JsonPatchException("a patch operation must be an object, but was $operation")

        val op = fields.requireString("op")
        val path = JsonPointer.tokens(fields.requireString("path"))

        return when (op) {
            "add" -> add(document, path, fields.require("value"))
            "replace" -> replace(document, path, fields.require("value"))
            "remove" -> remove(document, path)
            "test" -> {
                val actual = read(document, path)
                val expected = fields.require("value")
                if (actual != expected) {
                    throw JsonPatchException("test failed at '${fields.requireString("path")}': expected $expected but found $actual")
                }
                document
            }
            "move" -> {
                val from = JsonPointer.tokens(fields.requireString("from"))
                val value = read(document, from)
                add(remove(document, from), path, value)
            }
            "copy" -> add(document, path, read(document, JsonPointer.tokens(fields.requireString("from"))))
            // Refusing an unknown operation is the whole point: silently ignoring
            // one would leave the client rendering a configuration that is not
            // the one the agent thinks it sent, with nothing to show for it.
            else -> throw JsonPatchException("unsupported patch operation '$op'")
        }
    }

    private fun read(document: JsonElement, path: List<String>): JsonElement {
        var current = document
        for (token in path) {
            current = when (current) {
                is JsonObject -> current[token]
                    ?: throw JsonPatchException("no such key '${JsonPointer.format(listOf(token))}'")

                is JsonArray -> {
                    val index = arrayIndex(current, token, allowEnd = false)
                    current[index]
                }

                else -> throw JsonPatchException("cannot index into $current with '$token'")
            }
        }
        return current
    }

    private fun add(document: JsonElement, path: List<String>, value: JsonElement): JsonElement {
        if (path.isEmpty()) {
            return value
        }

        val parentPath = path.dropLast(1)
        val token = path.last()
        val parent = read(document, parentPath)

        return when (parent) {
            is JsonObject -> JsonObject(parent + (token to value))

            is JsonArray -> {
                // `-` means "append", per the spec.
                val index = if (token == "-") parent.size else arrayIndex(parent, token, allowEnd = true)
                JsonArray(parent.toMutableList().apply { add(index, value) })
            }

            else -> throw JsonPatchException("cannot add into $parent")
        }.let { updated -> replace(document, parentPath, updated) }
    }

    private fun replace(document: JsonElement, path: List<String>, value: JsonElement): JsonElement {
        if (path.isEmpty()) {
            return value
        }

        val parentPath = path.dropLast(1)
        val token = path.last()
        val parent = read(document, parentPath)

        val updated = when (parent) {
            is JsonObject -> {
                if (!parent.containsKey(token)) {
                    throw JsonPatchException("cannot replace '${JsonPointer.format(path)}': it does not exist")
                }
                JsonObject(parent + (token to value))
            }

            is JsonArray -> {
                val index = arrayIndex(parent, token, allowEnd = false)
                JsonArray(parent.toMutableList().apply { set(index, value) })
            }

            else -> throw JsonPatchException("cannot replace into $parent")
        }

        return replaceNested(document, parentPath, updated)
    }

    private fun remove(document: JsonElement, path: List<String>): JsonElement {
        if (path.isEmpty()) {
            throw JsonPatchException("cannot remove the whole document")
        }

        val parentPath = path.dropLast(1)
        val token = path.last()
        val parent = read(document, parentPath)

        val updated = when (parent) {
            is JsonObject -> {
                if (!parent.containsKey(token)) {
                    throw JsonPatchException("cannot remove '${JsonPointer.format(path)}': it does not exist")
                }
                JsonObject(parent - token)
            }

            is JsonArray -> {
                val index = arrayIndex(parent, token, allowEnd = false)
                JsonArray(parent.toMutableList().apply { removeAt(index) })
            }

            else -> throw JsonPatchException("cannot remove from $parent")
        }

        return replaceNested(document, parentPath, updated)
    }

    /**
     * Put `value` at `path`, creating the parents on the way down.
     *
     * Separate from [replace] because the parents here were just read, so they
     * must exist — this walks down by identity of token rather than re-checking.
     */
    private fun replaceNested(document: JsonElement, path: List<String>, value: JsonElement): JsonElement {
        if (path.isEmpty()) {
            return value
        }

        val parentPath = path.dropLast(1)
        val token = path.last()
        val parent = read(document, parentPath)

        val updated = when (parent) {
            is JsonObject -> JsonObject(parent + (token to value))
            is JsonArray -> {
                val index = arrayIndex(parent, token, allowEnd = false)
                JsonArray(parent.toMutableList().apply { set(index, value) })
            }
            else -> throw JsonPatchException("cannot write into $parent")
        }

        return replaceNested(document, parentPath, updated)
    }

    private fun arrayIndex(array: JsonArray, token: String, allowEnd: Boolean): Int {
        val index = token.toIntOrNull()
            ?: throw JsonPatchException("'$token' is not an array index")

        val upperBound = if (allowEnd) array.size else array.size - 1
        if (index < 0 || index > upperBound) {
            throw JsonPatchException("index $index is outside the array (size ${array.size})")
        }
        return index
    }
}

private fun JsonObject.require(key: String): JsonElement =
    this[key] ?: throw JsonPatchException("a patch operation must have a '$key'")

private fun JsonObject.requireString(key: String): String {
    val value = require(key)
    // proto3 JSON *omits* defaults, so an empty string arrives absent — which
    // means a missing key here is a malformed operation, not an empty one.
    val primitive = value as? JsonPrimitive
        ?: throw JsonPatchException("'$key' must be a string, but was $value")
    if (primitive is JsonNull) {
        throw JsonPatchException("'$key' must be a string, but was null")
    }
    return primitive.content
}
