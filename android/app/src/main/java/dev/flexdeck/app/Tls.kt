package dev.flexdeck.app

import android.content.SharedPreferences
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

/**
 * Trusts the agent's self-signed certificate, on first use, and remembers it.
 *
 * The certificate is self-signed, so there is no chain and no authority to check
 * it against — the *fingerprint* is what establishes identity, which is the
 * design rather than a workaround.
 *
 * "Trust on first use" is the honest name for what this does, and it has a real
 * weakness worth stating: the very first connection could be intercepted. The QR
 * flow exists precisely to avoid that, because the QR carries the fingerprint
 * and the client can compare it before ever connecting. Pairing by PIN has no
 * such channel, so this is the best that flow can do — and it is still much
 * better than trusting anything forever, because a *changed* certificate is
 * refused rather than quietly accepted.
 */
object Tls {

    /** The certificate the agent presented the first time we spoke to it. */
    private const val KEY_FINGERPRINT = "agentCertificateFingerprint"

    fun sslContext(preferences: SharedPreferences): SSLContext {
        val manager = PinnedTrustManager(preferences)
        val context = SSLContext.getInstance("TLS")
        context.init(null, arrayOf<TrustManager>(manager), null)
        return context
    }

    fun trustManager(preferences: SharedPreferences): X509TrustManager = PinnedTrustManager(preferences)

    /** The fingerprint this tablet has pinned, or null if it has never connected. */
    fun pinnedFingerprint(preferences: SharedPreferences): String? =
        preferences.getString(KEY_FINGERPRINT, null)

    /** Forget the pin, so the next connection learns a new certificate. */
    fun forget(preferences: SharedPreferences) {
        preferences.edit().remove(KEY_FINGERPRINT).apply()
    }

    /** Put back a pin set aside with [pinnedFingerprint], when a switch to another PC is abandoned. */
    fun restore(preferences: SharedPreferences, fingerprint: String) {
        preferences.edit().putString(KEY_FINGERPRINT, fingerprint).apply()
    }

    private class PinnedTrustManager(private val preferences: SharedPreferences) : X509TrustManager {

        override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {
            val leaf = chain?.firstOrNull()
                ?: throw CertificateException("the agent presented no certificate")

            val fingerprint = fingerprintOf(leaf)
            val pinned = preferences.getString(KEY_FINGERPRINT, null)

            if (pinned == null) {
                // First contact: this is the moment the pin is learned.
                preferences.edit().putString(KEY_FINGERPRINT, fingerprint).apply()
                return
            }

            if (pinned != fingerprint) {
                // Refusing is the entire value of pinning. A silently accepted
                // change would make the pin decorative.
                throw CertificateException(
                    "the agent's certificate has changed. If it was re-installed, " +
                        "clear this app's data and pair again.",
                )
            }
        }

        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) = Unit

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    private fun fingerprintOf(certificate: X509Certificate): String =
        MessageDigest.getInstance("SHA-256")
            .digest(certificate.encoded)
            .joinToString(separator = "") { byte -> "%02x".format(byte) }
}
