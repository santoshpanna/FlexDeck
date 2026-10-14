package dev.flexdeck.client

import kotlinx.serialization.json.JsonObject

/**
 * What this client currently holds, and how a frame changes it.
 *
 * Kept out of the Android code because the rules here are where a client
 * actually goes wrong: applying a patch against the wrong revision, or carrying
 * on after one failed to apply and rendering a document that never existed. Both
 * look like a working screen showing the wrong thing, which is the worst way for
 * a bug to present itself.
 */
/**
 * What a widget is showing right now: one of its looks, its live label, and —
 * for a slider — the value the agent last reported, `null` when unknown.
 */
data class LiveWidget(
    val stateId: String,
    val label: String,
    val value: Double? = null,
    val icon: String = "",
    /** Each face element's live value, by its index in the face that shows. */
    val elements: Map<Int, ElementLive> = emptyMap(),
    val background: String = "",
    val pulse: Boolean = false,
    val backgroundImage: String = "",
    /** Why it may be stale; empty when all is well. */
    val problem: String = "",
    /** Zones of its strip showing their other icon. */
    val zonesOn: Set<String> = emptySet(),
    /** Why it cannot do what it does any more; empty when it can. */
    val unavailable: String = "",
)

class DeckSession {

    private val live = mutableMapOf<String, LiveWidget>()

    /**
     * Widgets bound to live values, by id: lit or not, and their live label.
     *
     * Kept apart from [config] on purpose — it is what the widgets *show*, not
     * what the deck *is*, and a stale light must never be mistaken for a
     * configuration change.
     */
    val liveWidgets: Map<String, LiveWidget> get() = live

    private val history = mutableMapOf<String, ArrayDeque<Double>>()

    /** Each graphed value's recent history, oldest first. */
    val series: Map<String, List<Double>> get() = history

    var config: JsonObject? = null
        private set

    var rev: Long = 0
        private set

    /**
     * Set when a patch could not be applied.
     *
     * The client is then holding a document it cannot trust, and the only
     * correct move is to ask for the whole thing again. Continuing to apply
     * patches to a document that is already wrong just compounds the error in a
     * way nothing downstream can detect.
     */
    var needsResync: Boolean = false
        private set

    /** Whether there is a configuration to draw. */
    val hasConfig: Boolean get() = config != null

    fun apply(message: ServerMessage) {
        when (message) {
            is ServerMessage.Welcome -> {
                config = message.config
                rev = message.rev
                needsResync = false
            }

            is ServerMessage.Patch -> applyPatch(message)

            // Widget state is cosmetic, and deliberately never touches what the
            // client believes the configuration is. Every state frame carries
            // the widget's whole current look — a press answers with it too —
            // so the latest one simply wins.
            is ServerMessage.State -> {
                // A frame without a value (a failure report, say) must not
                // forget the one we had: the thumb would jump.
                val value = message.value ?: live[message.widgetId]?.value
                live[message.widgetId] = LiveWidget(
                    message.stateId,
                    message.label,
                    value,
                    message.icon,
                    message.elements.associateBy { it.index },
                    message.background,
                    message.pulse,
                    message.backgroundImage,
                    message.problem,
                    message.zonesOn,
                    message.unavailable,
                )
            }

            is ServerMessage.Failure -> needsResync = true
            is ServerMessage.Series -> {
                val points = if (message.append) history.getOrPut(message.variable) { ArrayDeque() } else ArrayDeque<Double>().also { history[message.variable] = it }
                points.addAll(message.points)
                while (points.size > MAX_POINTS) points.removeFirst()
            }
            // Which page shows is the view's to keep; nothing is held here.
            is ServerMessage.Page -> Unit
        }
    }

    private fun applyPatch(message: ServerMessage.Patch) {
        // A patch older than what we hold would move the document backwards.
        // That happens legitimately when an undo is broadcast, and the correct
        // response is to wait for the frame that carries the revision we want —
        // not to apply this one.
        if (message.rev <= rev) {
            return
        }

        val current = config
        if (current == null) {
            // A patch for a document we never received. Ask for it instead.
            needsResync = true
            return
        }

        val updated = try {
            JsonPatch.apply(current, message.patch)
        } catch (cause: JsonPatchException) {
            // The document is now in an unknown state relative to the agent.
            needsResync = true
            return
        }

        if (updated !is JsonObject) {
            needsResync = true
            return
        }

        config = updated
        rev = message.rev
    }

    /** Forget everything, for when the agent cannot be trusted to be in step. */
    fun reset() {
        config = null
        rev = 0
        needsResync = false
        // The agent sends every bound state again on reconnect; keeping old
        // ones would show a light from before the connection dropped.
        live.clear()
        history.clear()
    }

    private companion object {
        /** As the agent keeps: ten minutes at one a second. */
        const val MAX_POINTS = 600
    }
}
