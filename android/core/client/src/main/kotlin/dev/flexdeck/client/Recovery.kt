package dev.flexdeck.client

import java.security.cert.CertificateException

/**
 * When a failure is worth retrying, and when it is a decision.
 *
 * A dropped socket and a refusal look identical from a connection — both end it —
 * but they need opposite responses:
 *
 * * a drop is worth retrying forever, because the agent may simply be asleep; and
 * * a refusal means the agent has *decided*, so the next attempt will be refused
 *   in exactly the same way.
 *
 * Retrying a refusal is not merely pointless: it turns a fixable problem into a
 * screen that says "Reconnecting" indefinitely, with the real reason scrolling
 * past once a second. A panel screwed to a wall cannot be left in that state,
 * because there is no keyboard to type a fix into.
 *
 * The distinction is made here rather than inside the socket listener so it can
 * be tested without a socket, which is where this kind of rule is usually left
 * untested and wrong.
 */
object Recovery {

    /**
     * How far to follow a cause chain before giving up.
     *
     * The interesting value is never at the top: a TLS failure arrives as an
     * `SSLHandshakeException` that *causes* a `CertificateException`. Bounded
     * rather than "until there is no cause", because a throwable that is its own
     * cause is legal and this must always terminate.
     */
    private const val MAX_CAUSE_HOPS = 8

    /**
     * The certificate problem inside a connection failure, if there is one.
     *
     * A changed certificate is terminal by design — pinning is only worth
     * something if a change is refused — and no amount of retrying can clear a
     * pin. Only a person can, so this is what turns an endless reconnect into an
     * invitation to pair again.
     */
    fun certificateProblem(failure: Throwable?): CertificateException? {
        var current = failure
        var hops = 0

        while (current != null && hops < MAX_CAUSE_HOPS) {
            if (current is CertificateException) {
                return current
            }

            if (current.cause === current) {
                return null
            }

            current = current.cause
            hops++
        }

        return null
    }
}
