package dev.flexdeck.client

/**
 * Whether a finger on a key of a paged deck is a press or the start of a swipe.
 *
 * A key that fires the moment it is touched would already have run by the time
 * a swipe is recognised, so on a deck with pages the press is held back for a
 * moment. Within it, a clear sideways run makes it a swipe and nothing is sent;
 * a lift, a second finger, or the moment passing makes it a press. Once it is a
 * press it stays one, however much the finger wobbles afterwards.
 *
 * Pure: time comes in as `now`, and the timer is the caller's — it asks
 * [nextDeadline] when to call [tick]. Settles at most once.
 */
class PressGate(private val delayMs: Long = Pager.PRESS_DELAY_MS) {
    enum class Outcome { NONE, COMMIT, DISCARD }

    private var deadline: Long? = null

    /** Whether it is still undecided. */
    val pending: Boolean get() = deadline != null

    fun down(now: Long) {
        deadline = now + delayMs
    }

    /**
     * The finger is (`dx`, `dy`) from where it landed; `slop` is how far a swipe
     * from a key must go. A move that arrives after the moment has passed — the
     * timer not yet run — settles as the press it already is.
     */
    fun move(dx: Float, dy: Float, slop: Float, now: Long): Outcome {
        if (tick(now) == Outcome.COMMIT) return Outcome.COMMIT
        return if (pending && Pager.claims(dx, dy, slop)) settle(Outcome.DISCARD) else Outcome.NONE
    }

    fun tick(now: Long): Outcome {
        val due = deadline ?: return Outcome.NONE
        return if (now >= due) settle(Outcome.COMMIT) else Outcome.NONE
    }

    /** Lifted before the moment passed: a quick tap. */
    fun up(): Outcome = if (pending) settle(Outcome.COMMIT) else Outcome.NONE

    /** A second finger makes it a chord, not a swipe. */
    fun otherFinger(): Outcome = if (pending) settle(Outcome.COMMIT) else Outcome.NONE

    /** The gesture was taken away: nothing it began is sent. */
    fun cancel(): Outcome = if (pending) settle(Outcome.DISCARD) else Outcome.NONE

    fun nextDeadline(): Long? = deadline

    private fun settle(outcome: Outcome): Outcome {
        deadline = null
        return outcome
    }
}
