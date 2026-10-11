package dev.flexdeck.client

import javax.net.ssl.SSLHandshakeException
import java.io.IOException
import java.security.cert.CertificateException
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertSame

class RecoveryTest {

    @Test
    fun `a bare certificate failure is found`() {
        val problem = CertificateException("the agent's certificate has changed")

        assertSame(problem, Recovery.certificateProblem(problem))
    }

    @Test
    fun `a wrapped certificate failure is found`() {
        // The real shape: TLS wraps the certificate error, and the wrapper is
        // what the socket listener is handed.
        val problem = CertificateException("the agent's certificate has changed")
        val wrapped = SSLHandshakeException("handshake failed").apply { initCause(problem) }

        assertSame(problem, Recovery.certificateProblem(wrapped))
    }

    @Test
    fun `a deeply wrapped certificate failure is still found`() {
        val problem = CertificateException("changed")
        val middle = IOException("the connection could not be established").apply { initCause(problem) }
        val outer = IOException("a request failed").apply { initCause(middle) }

        assertSame(problem, Recovery.certificateProblem(outer))
    }

    @Test
    fun `an ordinary network failure is not a certificate problem`() {
        // This is the case that MUST keep retrying: the agent is asleep, or the
        // tablet drifted off the network.
        val dropped = IOException("failed to connect to /192.168.1.6:7878")

        assertNull(Recovery.certificateProblem(dropped))
        assertNull(Recovery.certificateProblem(null))
    }

    @Test
    fun `a throwable that causes itself terminates`() {
        // Legal in Java, and a `while (cause != null)` walk would spin forever on
        // it — which would hang the socket callback rather than failing it.
        val loop = object : Throwable("I am my own cause") {
            override val cause: Throwable get() = this
        }

        assertNull(Recovery.certificateProblem(loop))
    }

    @Test
    fun `a chain longer than the cap gives up rather than walking forever`() {
        var deep: Throwable = CertificateException("buried too far down")

        repeat(RecoveryTest.UNREACHABLE_DEPTH) {
            deep = IOException("layer").apply { initCause(deep) }
        }

        // The cap is a deliberate trade: refusing to walk an absurd chain is
        // better than a listener that never returns.
        assertNull(Recovery.certificateProblem(deep))
    }

    private companion object {
        /** Comfortably past `Recovery`'s own limit. */
        const val UNREACHABLE_DEPTH = 12
    }
}
