package dev.flexdeck.app

import dev.flexdeck.client.Frames
import dev.flexdeck.client.Recovery
import dev.flexdeck.client.ServerMessage
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import okhttp3.Call
import okhttp3.Callback
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.io.IOException
import java.util.concurrent.TimeUnit

/**
 * Talks to the agent: one REST call to pair, then a WebSocket for everything
 * after.
 *
 * All parsing goes through `Frames` rather than being redone here, so the
 * awkward parts of this protocol — the proto3 JSON mapping's omissions, and the
 * patch that arrives double-encoded — are handled in exactly one place, the one
 * that has tests.
 */
class AgentClient(
    private val preferences: android.content.SharedPreferences,
    private val listener: Listener,
) {

    interface Listener {
        fun onFrame(message: ServerMessage)
        fun onSocketUp()
        fun onSocketDown(reason: String)

        /**
         * The agent refused this tablet, or its certificate is not the one this
         * tablet pinned.
         *
         * Deliberately not `onSocketDown`: neither of these fixes itself by
         * trying again, and treating them as an ordinary drop is what leaves the
         * panel saying "Reconnecting" forever with the reason flashing past.
         */
        fun onRefused(code: String, message: String)
    }

    private val http = OkHttpClient.Builder()
        // The agent serves TLS with its own self-signed certificate, and this
        // client pins it. Every request carries a bearer token, so plain HTTP
        // would put that token on the LAN in the clear.
        .sslSocketFactory(Tls.sslContext(preferences).socketFactory, Tls.trustManager(preferences))
        // The certificate names `localhost`, which is never the address a tablet
        // dials. Checking it would reject every connection; the *fingerprint* is
        // what actually establishes identity here, and it is checked above.
        .hostnameVerifier { _, _ -> true }
        // The agent may be asleep or the tablet may have drifted off the network.
        // Failing fast and reconnecting is friendlier than hanging on a dead
        // socket for two minutes.
        .connectTimeout(5, TimeUnit.SECONDS)
        // A WebSocket is idle between pushes by design, so it must not have a
        // read timeout: the ping is what proves the far end is still there.
        .readTimeout(0, TimeUnit.MILLISECONDS)
        // The ONLY heartbeat in the system. The agent deliberately no longer
        // sends one, because a ping from the agent forces a radio wake-up on this
        // tablet whether or not anything has changed — and this app is meant to
        // be left on a wall for hours. One ping from the side that has a battery
        // is enough: the agent's liveness window is 75s (LIVENESS_SECS in
        // crates/flexdeck-server/src/ws.rs), so 30s tolerates two lost pings
        // before the agent gives up on us.
        .pingInterval(30, TimeUnit.SECONDS)
        .build()

    /**
     * The socket in use, or null when there is none.
     *
     * `@Volatile` because `close()` runs on the main thread while the callbacks
     * that read this arrive on OkHttp's. Every callback also checks that it is
     * still about *this* socket before reporting, which is what stops a
     * deliberate close from being read as a dropped connection.
     */
    @Volatile
    private var socket: WebSocket? = null

    /** Where the socket is connected, and with which token, for image requests. */
    @Volatile
    private var endpoint: Triple<String, Int, String>? = null

    /** The same pinned client, but a picture that takes ten seconds is not coming. */
    private val images = http.newBuilder().readTimeout(10, TimeUnit.SECONDS).build()

    /**
     * Fetch a picture the agent holds (`img:<sha256>` — album art), with this
     * tablet's token. [onLoaded] gets the bytes, or `null`, on OkHttp's thread.
     */
    fun fetchImage(sha256: String, onLoaded: (ByteArray?) -> Unit) {
        val (host, port, token) = endpoint ?: return onLoaded(null)
        val request = Request.Builder()
            .url("https://$host:$port/api/images/$sha256")
            .header("Authorization", "Bearer $token")
            .build()
        images.newCall(request).enqueue(object : okhttp3.Callback {
            override fun onFailure(call: okhttp3.Call, e: java.io.IOException) = onLoaded(null)
            override fun onResponse(call: okhttp3.Call, response: okhttp3.Response) {
                response.use {
                    val body = it.body
                    // Bounded: a picture for a button is never this big.
                    onLoaded(if (it.isSuccessful && body != null && body.contentLength() <= 4_000_000) body.bytes() else null)
                }
            }
        })
    }

    /**
     * Fetch an installed icon pack's icons (`GET /api/iconpacks/{id}`) with
     * this tablet's token. [onLoaded] gets the JSON text, or `null`.
     */
    fun fetchPack(id: String, onLoaded: (String?) -> Unit) {
        val (host, port, token) = endpoint ?: return onLoaded(null)
        val request = Request.Builder()
            .url("https://$host:$port/api/iconpacks/${java.net.URLEncoder.encode(id, "UTF-8")}")
            .header("Authorization", "Bearer $token")
            .build()
        images.newCall(request).enqueue(object : okhttp3.Callback {
            override fun onFailure(call: okhttp3.Call, e: java.io.IOException) = onLoaded(null)
            override fun onResponse(call: okhttp3.Call, response: okhttp3.Response) {
                response.use {
                    val body = it.body
                    onLoaded(if (it.isSuccessful && body != null && body.contentLength() <= 8_000_000) body.string() else null)
                }
            }
        })
    }

    /** Redeem a PIN for a device token. */
    fun pair(host: String, port: Int, pin: String, deviceName: String, onResult: (Result<String>) -> Unit) {
        val payload = buildJsonObject {
            put("pin", pin)
            put("deviceName", deviceName)
            // Pairing again from this install replaces its old entry on the PC
            // rather than adding a second one.
            put("clientId", installId())
        }.toString().toRequestBody(JSON)

        val request = Request.Builder()
            .url("https://$host:$port/api/pairing/redeem")
            .post(payload)
            .build()

        val call = http.newCall(request)

        // The socket client has an infinite read timeout on purpose and pairing
        // shares it, so the call itself is what has to be bounded. Without this,
        // an agent that accepts the connection and then says nothing leaves the
        // form saying "Pairing..." forever, with nothing to act on.
        call.timeout().timeout(PAIR_TIMEOUT_SECONDS, TimeUnit.SECONDS)

        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, failure: IOException) {
                    onResult(Result.failure(failure))
                }

                override fun onResponse(call: Call, response: Response) {
                    response.use {
                        val text = it.body?.string().orEmpty()
                        val root = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull()

                        if (runCatching { Frames.parsePairingGranted(text) }.getOrDefault(false)) {
                            val token = (root?.get("token") as? JsonPrimitive)?.content.orEmpty()
                            onResult(Result.success(token))
                        } else {
                            // A refusal carries the reason; an unreadable body
                            // does not, so say what actually happened.
                            val reason = (root?.get("error") as? JsonPrimitive)?.content
                            onResult(Result.failure(IllegalStateException(reason ?: "the agent refused the pairing")))
                        }
                    }
                }
            },
        )
    }

    /**
     * Ask the PC to drop this tablet from its device list, before the tablet
     * forgets the PC. Best effort: a PC that is off or unreachable keeps the
     * entry, and its owner can remove it there. [done] runs either way, on
     * OkHttp's thread.
     */
    fun forgetMe(host: String, port: Int, token: String, done: () -> Unit) {
        if (host.isBlank() || token.isBlank()) return done()
        val request = runCatching {
            Request.Builder()
                .url("https://$host:$port/api/devices/forget-me")
                .header("Authorization", "Bearer $token")
                .post("{}".toRequestBody(JSON))
                .build()
        }.getOrElse { return done() }
        val call = http.newCall(request)
        call.timeout().timeout(FORGET_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        call.enqueue(object : Callback {
            override fun onFailure(call: Call, e: IOException) = done()
            override fun onResponse(call: Call, response: Response) {
                response.close()
                done()
            }
        })
    }

    /** An id this install made up once and keeps. Not a secret. */
    private fun installId(): String =
        preferences.getString(KEY_INSTALL_ID, null) ?: java.util.UUID.randomUUID().toString().also {
            preferences.edit().putString(KEY_INSTALL_ID, it).apply()
        }

    /**
     * Open the socket and say hello.
     *
     * Any previous socket is closed first, so there is exactly one live
     * connection. The callbacks all check that they are still about *that*
     * socket, which is what keeps a deliberate close from being mistaken for a
     * dropped connection and manufacturing a reconnect out of nothing.
     */
    fun connect(host: String, port: Int, token: String) {
        endpoint = Triple(host, port, token)
        close()

        val request = try {
            Request.Builder().url("wss://$host:$port/ws").build()
        } catch (cause: IllegalArgumentException) {
            // A host that will not make a URL is something only a person can fix.
            // Leaking the exception would take the app down instead of putting
            // the reason on the status line.
            listener.onSocketDown("that agent address cannot be used: $host:$port")
            return
        }

        socket = http.newWebSocket(
            request,
            object : WebSocketListener() {
                /**
                 * Whether this callback is still about the socket in use.
                 *
                 * After a close or a replacement the field points at something
                 * else (or nothing), and a late callback from the old socket must
                 * not be read as the current connection dropping.
                 */
                private fun isCurrent(webSocket: WebSocket): Boolean = socket === webSocket

                override fun onOpen(webSocket: WebSocket, response: Response) {
                    if (!isCurrent(webSocket)) return

                    webSocket.send(
                        buildJsonObject {
                            putJsonObject("hello") {
                                put("role", "android")
                                put("token", token)
                            }
                        }.toString(),
                    )
                    listener.onSocketUp()
                }

                override fun onMessage(webSocket: WebSocket, text: String) {
                    if (!isCurrent(webSocket)) return

                    val message = runCatching { Frames.parseServerMessage(text) }
                        // An unreadable frame means the two sides disagree about
                        // the protocol. Dropping the connection and reconnecting
                        // is better than carrying on with a stale screen.
                        .getOrElse {
                            listener.onSocketDown(it.message ?: "the agent sent something unreadable")
                            return
                        }

                    if (message is ServerMessage.Failure) {
                        // A refusal is a decision, not a dropped packet. The agent
                        // closes the socket straight after, so reporting this as a
                        // plain disconnect would make the tablet retry a rejection
                        // in a loop.
                        listener.onRefused(message.code, message.message)
                    } else {
                        listener.onFrame(message)
                    }
                }

                override fun onFailure(webSocket: WebSocket, failure: Throwable, response: Response?) {
                    if (!isCurrent(webSocket)) return

                    val certificate = Recovery.certificateProblem(failure)

                    if (certificate != null) {
                        // Only a person can clear a pin, and the pin is checked
                        // before the socket is even open — so retrying is not a
                        // slow path to recovery, it is no path at all.
                        listener.onRefused(
                            "tls",
                            certificate.message
                                ?: "the agent's certificate is not the one this tablet pinned",
                        )
                    } else {
                        listener.onSocketDown(failure.message ?: "the connection failed")
                    }
                }

                override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                    if (!isCurrent(webSocket)) return

                    listener.onSocketDown(reason.ifBlank { "the agent closed the connection" })
                }
            },
        )
    }

    /** Tell the agent a widget was touched, and how. */
    fun sendPress(widgetId: String, gesture: String = "GESTURE_SHORT_PRESS", zone: String = "") {
        val payload = buildJsonObject {
            putJsonObject("press") {
                put("widgetId", widgetId)
                put("gesture", gesture)
                if (zone.isNotEmpty()) put("zone", zone)
            }
        }.toString()

        // Read once: `socket` is volatile and could be replaced between a null
        // check and the send.
        val open = socket ?: return
        open.send(payload)
    }

    /**
     * Tell the agent a slider moved. [drag] increases per drag; [lifted] marks
     * the value the finger let go on. The view has already paced these.
     */
    fun sendValue(widgetId: String, value: Double, drag: Int, lifted: Boolean) {
        val payload = buildJsonObject {
            putJsonObject("value") {
                put("widgetId", widgetId)
                put("value", value)
                put("seq", drag)
                // proto3 JSON omits false; sending it is harmless and clearer.
                put("final", lifted)
            }
        }.toString()
        val open = socket ?: return
        open.send(payload)
    }

    /**
     * Tell the agent which page is on screen, so the editor and other tablets
     * follow. [initial] marks the report made on connecting.
     */
    fun sendPage(pageId: String, initial: Boolean = false) {
        val payload = buildJsonObject {
            putJsonObject("page") {
                put("pageId", pageId)
                put("initial", initial)
            }
        }.toString()
        val open = socket ?: return
        open.send(payload)
    }

    /**
     * Drop the connection, without reporting it as a dropped connection.
     *
     * The field is detached BEFORE the socket is closed, so the callbacks that
     * follow see that they are no longer current and stay quiet. Otherwise every
     * deliberate close — a resync, a backgrounded app, a re-pair — would look
     * exactly like the agent going away, and would schedule a reconnect from the
     * one place that was trying to stop one.
     */
    fun close() {
        val open = socket
        socket = null
        open?.close(1000, null)
    }

    private companion object {
        val JSON = "application/json; charset=utf-8".toMediaType()

        /** How long the one pairing call may take, end to end. */
        const val PAIR_TIMEOUT_SECONDS = 15L

        /** Disconnecting waits this long for the PC at most, then goes anyway. */
        const val FORGET_TIMEOUT_SECONDS = 4L

        const val KEY_INSTALL_ID = "installId"
    }
}
