package dev.flexdeck.client

import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject

/**
 * The messages the agent sends.
 *
 * Parsed by hand from `JsonElement` rather than generated from the schema, for
 * one specific reason: the wire format is the **proto3 JSON mapping**, which
 * *omits default values*. A `false` boolean or a `0` number is simply not in the
 * payload. A generated deserialiser hides that behind a default; doing it by
 * hand makes every omission a visible decision, and the omissions are exactly
 * where a client goes wrong.
 *
 * A missing `ok` therefore means `false`, a missing `rev` means `0`, and neither
 * is an error.
 */
sealed interface ServerMessage {

    /** The whole configuration, sent once when a client connects. */
    data class Welcome(val rev: Long, val profileId: String, val config: JsonObject?) : ServerMessage

    /**
     * A change to apply to the configuration the client already holds.
     *
     * The patch arrives **double-encoded** — a JSON string inside a JSON message
     * — so it is parsed here, once, rather than being handed on as a string for
     * every caller to remember to parse. Forgetting that second parse is the
     * easiest mistake in this protocol and it fails silently.
     */
    data class Patch(val rev: Long, val patch: JsonArray) : ServerMessage

    /**
     * What a widget shows now — after a press, or because something it follows
     * moved. `stateId` names one of the widget's looks, empty for "its own
     * props"; `label` is its live label, empty for "the look's or configured
     * one".
     */
    data class State(
        val widgetId: String,
        val stateId: String = "",
        val label: String = "",
        val ok: Boolean = true,
        val error: String = "",
        /** A slider's value. Absent — not zero — when nobody knows it yet. */
        val value: Double? = null,
        /** An icon that follows a value; empty for "the configured one". */
        val icon: String = "",
        /** Schema 4: each element of the face that shows, by its index there. */
        val elements: List<ElementLive> = emptyList(),
        /** The background colour a face rule picked; empty for the configured one. */
        val background: String = "",
        /** A face rule asks the whole key to pulse. */
        val pulse: Boolean = false,
        /** The face's background picture, its template filled; empty for none. */
        val backgroundImage: String = "",
        /** Why what it shows may be stale — a source it reads is failing; empty for none. */
        val problem: String = "",
        /** Zones of its strip whose `when` holds: they show their other icon. */
        val zonesOn: Set<String> = emptySet(),
        /** Why it cannot do what it does any more — a plugin lost an action it calls; empty when it can. */
        val unavailable: String = "",
    ) : ServerMessage

    /**
     * A graphed value's recent history: the whole of it (`append` false) or
     * the newest points to add to it. Evenly spaced, oldest first.
     */
    data class Series(val variable: String, val points: List<Double>, val append: Boolean) : ServerMessage

    /** Show a page: the one with `pageId`, or `step` pages on from this one. */
    data class Page(val pageId: String, val step: Int) : ServerMessage

    /** The agent refused something, such as a bad token. */
    data class Failure(val code: String, val message: String) : ServerMessage
}

/** One face element's live value. */
data class ElementLive(
    val index: Int,
    /** A text's filled template, or an icon's name or `img:<sha256>`. */
    val text: String = "",
    /** A bar's or gauge's value; `null` while unknown. */
    val number: Double? = null,
    /** The colour a rule picked; empty for the configured one. */
    val color: String = "",
    val pulse: Boolean = false,
) {
    /** When it arrived, by this tablet's clock: a running timer counts down from here. */
    val receivedAt: Long = System.currentTimeMillis()
}

class FrameException(message: String) : Exception(message)

object Frames {

    private val json = Json { ignoreUnknownKeys = true }

    /**
     * Parse one frame from the agent.
     *
     * Throws rather than returning null: a frame that cannot be understood means
     * the client and the agent disagree about the protocol, and carrying on with
     * the previous configuration while pretending nothing happened is how that
     * becomes an unexplained stale screen.
     */
    fun parseServerMessage(text: String): ServerMessage {
        val root = parseObject(text)

        // The oneof is encoded by field name, so exactly one of these is present.
        root["welcome"]?.let { return parseWelcome(it.jsonObject) }
        root["patch"]?.let { return parsePatch(it.jsonObject) }
        root["state"]?.let { return parseState(it.jsonObject) }
        root["error"]?.let { return parseFailure(it.jsonObject) }
        root["series"]?.let { return parseSeries(it.jsonObject) }
        root["page"]?.let { return parsePage(it.jsonObject) }

        throw FrameException("no recognised message kind in $text")
    }

    /** Parse the payload the agent returns from `POST /api/pairing/redeem`. */
    fun parsePairingGranted(text: String): Boolean {
        val root = parseObject(text)
        // Absent means false, per the proto3 JSON mapping.
        return (root["ok"] as? JsonPrimitive)?.content?.toBoolean() ?: false
    }

    /**
     * Parse a frame into a JSON object, translating the JSON library's own
     * exception types.
     *
     * Callers should not have to know that kotlinx-serialization exists, and
     * `catch (e: SerializationException)` scattered around a client is how a
     * dependency leaks into every layer above it.
     */
    private fun parseObject(text: String): JsonObject =
        try {
            json.parseToJsonElement(text).jsonObject
        } catch (cause: SerializationException) {
            throw FrameException("the frame was not valid JSON: ${cause.message}")
        } catch (cause: IllegalArgumentException) {
            // Parsed, but not an object — an array or a bare string.
            throw FrameException("the frame was not a JSON object: ${cause.message}")
        }

    private fun parseWelcome(fields: JsonObject): ServerMessage.Welcome =
        ServerMessage.Welcome(
            rev = fields.long("rev"),
            // Absent means empty, and this build does not choose profiles yet.
            profileId = fields.string("profileId"),
            config = fields["config"]?.jsonObject,
        )

    private fun parsePatch(fields: JsonObject): ServerMessage.Patch {
        val encoded = fields.string("patch")

        // The inner string is the actual RFC 6902 array. A patch that is not an
        // array is a protocol error, not an empty patch.
        val operations = runCatching { json.parseToJsonElement(encoded) as? JsonArray }.getOrNull()
            ?: throw FrameException("the patch payload was not a JSON array: $encoded")

        return ServerMessage.Patch(rev = fields.long("rev"), patch = operations)
    }

    private fun parseState(fields: JsonObject): ServerMessage.State =
        ServerMessage.State(
            widgetId = fields.string("widgetId"),
            stateId = fields.string("stateId"),
            label = fields.string("label"),
            // Absent means false in proto3 JSON, but an absent `ok` on a state
            // frame from an older agent meant success, so only an explicit
            // `false` is a failure.
            ok = (fields["ok"] as? JsonPrimitive)?.content?.toBooleanStrictOrNull() ?: true,
            error = fields.string("error"),
            value = (fields["value"] as? JsonPrimitive)?.content?.toDoubleOrNull(),
            icon = fields.string("icon"),
            elements = (fields["elements"] as? JsonArray).orEmpty().mapNotNull { element ->
                val item = element as? JsonObject ?: return@mapNotNull null
                ElementLive(
                    index = item.long("index").toInt(),
                    text = item.string("text"),
                    number = (item["number"] as? JsonPrimitive)?.content?.toDoubleOrNull(),
                    color = item.string("color"),
                    pulse = item.bool("pulse"),
                )
            },
            background = fields.string("background"),
            pulse = fields.bool("pulse"),
            backgroundImage = fields.string("backgroundImage"),
            problem = fields.string("problem"),
            zonesOn = (fields["zonesOn"] as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.content }.toSet(),
            unavailable = fields.string("unavailable"),
        )

    private fun parseSeries(fields: JsonObject): ServerMessage.Series =
        ServerMessage.Series(
            variable = fields.string("variable"),
            // proto3 JSON may write a double as a string ("NaN", "1e400"):
            // whatever is not a finite number is left out.
            points = (fields["points"] as? JsonArray).orEmpty().mapNotNull { point ->
                (point as? JsonPrimitive)?.content?.toDoubleOrNull()?.takeIf { it.isFinite() }
            },
            append = fields.bool("append"),
        )

    private fun parsePage(fields: JsonObject): ServerMessage.Page =
        ServerMessage.Page(pageId = fields.string("pageId"), step = fields.long("step").toInt())

    private fun parseFailure(fields: JsonObject): ServerMessage.Failure =
        ServerMessage.Failure(
            code = fields.string("code"),
            message = fields.string("message"),
        )

    private fun JsonObject.string(key: String): String =
        (this[key] as? JsonPrimitive)?.content ?: ""

    private fun JsonObject.bool(key: String): Boolean =
        (this[key] as? JsonPrimitive)?.content?.toBooleanStrictOrNull() ?: false

    private fun JsonObject.long(key: String): Long =
        (this[key] as? JsonPrimitive)?.content?.toLongOrNull() ?: 0L
}
