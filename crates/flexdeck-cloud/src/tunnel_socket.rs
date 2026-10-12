//! The desktop-facing WebSocket: the tunnel endpoint.
//!
//! # A fingerprint is claimed, then proved
//!
//! The cloud routes by fingerprint, and a fingerprint is not a secret — every
//! tablet paired with a desktop knows its own. So the cloud speaks first, with a
//! fresh random challenge, and the desktop's hello must carry its certificate
//! and a signature over that challenge by the certificate's key. Only then is
//! the tunnel attached. Knowing a desktop's fingerprint gets an impostor a
//! refusal; it does not get the desktop's traffic.
//!
//! # The handshake is the security decision, and it is a pure function
//!
//! [`admit`] decides whether a tunnel may proceed, given the challenge and the
//! hello. Keeping it out of the socket loop is the same split the agent uses for
//! its device-token check: a test that needed a live WebSocket to ask "is a bad
//! signature refused?" would be slower, flakier, and no more convincing.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use flexdeck_protocol::{
    tunnel_frame, TunnelChallenge, TunnelFrame, TunnelHello, TunnelRejected, TunnelWelcome,
};
use flexdeck_tunnel::{decode, encode, PROTOCOL_VERSION};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};

use crate::cloud::{now_secs, Cloud};

/// How long a new connection has to answer the challenge.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a tunnel may go without a single frame before it is treated as gone.
///
/// The desktop pings every 30 seconds, so this tolerates two missed pings. A
/// tunnel that looks open but is not would leave the website reporting a
/// desktop as online while every request to it times out.
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// The largest frame a desktop may send. Relayed bodies are capped at 1 MiB by
/// the agent; this leaves room for the envelope and nothing much more.
const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

type Sink = SplitSink<WebSocket, Message>;
type Stream = SplitStream<WebSocket>;

/// The socket half of the tunnel.
pub fn router(cloud: Arc<Cloud>) -> Router {
    Router::new()
        .route("/tunnel", get(upgrade))
        .with_state(cloud)
}

async fn upgrade(State(cloud): State<Arc<Cloud>>, socket: WebSocketUpgrade) -> Response {
    socket
        .max_message_size(MAX_FRAME_BYTES)
        .max_frame_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| connection(cloud, socket))
}

/// What the handshake decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// The tunnel may proceed, as the desktop with this fingerprint.
    Welcome { fingerprint: String },

    /// The tunnel must be closed, and the desktop told why.
    Reject { code: String, message: String },
}

fn reject(code: &str, message: impl Into<String>) -> Admission {
    Admission::Reject {
        code: code.to_string(),
        message: message.into(),
    }
}

/// Decide whether a hello answers `nonce` for the fingerprint it claims.
pub fn admit(nonce: &str, hello: &TunnelHello) -> Admission {
    if hello.protocol_version != PROTOCOL_VERSION {
        return reject(
            "protocol_version",
            format!(
                "this server speaks tunnel protocol {PROTOCOL_VERSION}, the desktop sent {}; \
                 update the desktop app",
                hello.protocol_version
            ),
        );
    }

    let fingerprint = hello.fingerprint.trim();
    if fingerprint.is_empty() {
        return reject(
            "fingerprint",
            "a tunnel must present the fingerprint of its certificate",
        );
    }

    let message = flexdeck_pairing::tunnel_proof_message(nonce, fingerprint);
    if let Err(error) =
        flexdeck_pairing::verify_proof(&hello.certificate, fingerprint, &message, &hello.signature)
    {
        return reject(
            "proof",
            format!("this desktop did not prove its identity: {error}"),
        );
    }

    Admission::Welcome {
        fingerprint: fingerprint.to_string(),
    }
}

async fn connection(cloud: Arc<Cloud>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();

    // A slot first, before anything is spent on the connection. Held until the
    // socket ends, so the cap counts tunnels actually open.
    let Ok(_slot) = cloud.tunnel_slots().try_acquire_owned() else {
        let _ = send_frame(
            &mut sink,
            &rejected("busy", "too many desktops are connected"),
        )
        .await;
        return;
    };

    let nonce = flexdeck_pairing::generate_token();
    let challenge = TunnelFrame {
        kind: Some(tunnel_frame::Kind::Challenge(TunnelChallenge {
            nonce: nonce.clone(),
        })),
    };
    if !send_frame(&mut sink, &challenge).await {
        return;
    }

    let hello = match tokio::time::timeout(HELLO_TIMEOUT, await_hello(&mut stream)).await {
        Ok(Some(hello)) => hello,
        Ok(None) => {
            let frame = rejected(
                "hello_expected",
                "the answer to the challenge must be a hello",
            );
            let _ = send_frame(&mut sink, &frame).await;
            return;
        }
        Err(_) => {
            let frame = rejected("hello_expected", "no hello arrived in time");
            let _ = send_frame(&mut sink, &frame).await;
            return;
        }
    };

    match admit(&nonce, &hello) {
        Admission::Reject { code, message } => {
            let _ = send_frame(&mut sink, &rejected(&code, &message)).await;
        }

        Admission::Welcome { fingerprint } => {
            let frame = TunnelFrame {
                kind: Some(tunnel_frame::Kind::Welcome(TunnelWelcome {})),
            };

            if send_frame(&mut sink, &frame).await {
                pump(cloud, fingerprint, hello.name, sink, stream).await;
            }
        }
    }
}

fn rejected(code: &str, message: &str) -> TunnelFrame {
    TunnelFrame {
        kind: Some(tunnel_frame::Kind::Rejected(TunnelRejected {
            code: code.to_string(),
            message: message.to_string(),
        })),
    }
}

/// Carry frames both ways until either end stops.
async fn pump(
    cloud: Arc<Cloud>,
    fingerprint: String,
    name: String,
    mut sink: Sink,
    mut stream: Stream,
) {
    // A proven key holder may replace its own earlier tunnel — that is just a
    // reconnect. The generation is what keeps the old connection's clean-up
    // below from removing this one.
    let (generation, mut frames) = cloud.hub().attach_generation(&fingerprint);
    cloud.hub().observe_name(&fingerprint, &name);

    // One deadline, pushed back only by what the DESKTOP sends. A timeout
    // re-armed every pass would also be reset by frames going down — and a
    // watching browser relays one every few seconds, so a dead tunnel behind a
    // proxy that never closed it would stay "online" for as long as anyone
    // looked at it.
    let idle = tokio::time::sleep(IDLE_TIMEOUT);
    tokio::pin!(idle);

    loop {
        tokio::select! {
            // Something to send down: a relayed request.
            outgoing = frames.recv() => {
                let Some(frame) = outgoing else {
                    // The hub dropped our sender: the tunnel was replaced, or
                    // detached. Either way this socket is over.
                    break;
                };
                if !send_frame(&mut sink, &frame).await {
                    break;
                }
            }

            // Nothing from the desktop for too long: it is gone.
            () = &mut idle => break,

            // Something the desktop sent.
            incoming = stream.next() => {
                let Some(Ok(message)) = incoming else { break };
                idle.as_mut().reset(tokio::time::Instant::now() + IDLE_TIMEOUT);

                let Ok(text) = message.to_text() else { continue };
                let Ok(frame) = decode(text) else {
                    // Both ends are our own software, so an unreadable frame is
                    // a version mismatch or a bug, and continuing would turn it
                    // into a request that never completes.
                    break;
                };

                if let Some(reply) = handle(&cloud, &fingerprint, frame, now_secs()) {
                    if !send_frame(&mut sink, &reply).await {
                        break;
                    }
                }
            }
        }
    }

    // Only this connection's own tunnel, and only then its pairing offer: if the
    // desktop has already reconnected, both belong to the new connection.
    if cloud.hub().detach_generation(&fingerprint, generation) {
        cloud.offers().withdraw(&fingerprint);
    }
}

/// Act on one frame from a desktop, returning anything to send back.
///
/// `pub(crate)` so the HTTP surface's tests can drive one frame directly instead
/// of standing up a whole WebSocket for it.
pub(crate) fn handle(
    cloud: &Cloud,
    fingerprint: &str,
    frame: TunnelFrame,
    now: i64,
) -> Option<TunnelFrame> {
    match frame.kind {
        Some(tunnel_frame::Kind::Response(response)) => {
            // The outcome is discarded on purpose: `Abandoned` just means the
            // browser gave up first, and `Unknown` means the desktop invented an
            // id. Neither is worth dropping a working tunnel over.
            let _ = cloud.hub().resolve(fingerprint, response);
            None
        }

        Some(tunnel_frame::Kind::Push(push)) => {
            if let Some(message) = push.message {
                cloud.hub().push(fingerprint, message);
            }
            None
        }

        // The desktop opened (or closed) a browser pairing code. Filed under the
        // tunnel's PROVEN fingerprint — the frame cannot name another desktop.
        Some(tunnel_frame::Kind::PairOffer(offer)) => {
            cloud
                .offers()
                .offer(fingerprint, &offer.code_hash, offer.expires_at, now);
            None
        }

        Some(tunnel_frame::Kind::Ping(_)) => Some(TunnelFrame {
            kind: Some(tunnel_frame::Kind::Pong(Default::default())),
        }),

        _ => None,
    }
}

/// The first frame after the challenge, which must be a hello.
async fn await_hello(stream: &mut Stream) -> Option<TunnelHello> {
    let message = stream.next().await?.ok()?;
    let text = message.to_text().ok()?;
    let frame = decode(text).ok()?;

    match frame.kind {
        Some(tunnel_frame::Kind::Hello(hello)) => Some(hello),
        _ => None,
    }
}

async fn send_frame(sink: &mut Sink, frame: &TunnelFrame) -> bool {
    let Ok(text) = encode(frame) else {
        return false;
    };

    sink.send(Message::text(text)).await.is_ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flexdeck_pairing::Identity;
    use flexdeck_protocol::PairOffer;

    fn identity() -> Identity {
        Identity::generate(vec!["localhost".to_string()]).expect("identity")
    }

    /// A hello as a genuine desktop holding `identity` would answer `nonce`.
    pub(crate) fn greeting(identity: &Identity, nonce: &str) -> TunnelHello {
        let fingerprint = identity.fingerprint();
        TunnelHello {
            protocol_version: PROTOCOL_VERSION,
            signature: identity
                .sign(&flexdeck_pairing::tunnel_proof_message(nonce, &fingerprint))
                .expect("signs"),
            fingerprint,
            name: "Studio PC".to_string(),
            rev: 0,
            certificate: identity.cert_der().to_vec(),
        }
    }

    fn code_of(admission: Admission) -> String {
        match admission {
            Admission::Reject { code, .. } => code,
            Admission::Welcome { .. } => panic!("expected a refusal"),
        }
    }

    #[test]
    fn a_desktop_that_proves_its_key_is_welcomed() {
        let identity = identity();
        assert_eq!(
            admit("n1", &greeting(&identity, "n1")),
            Admission::Welcome {
                fingerprint: identity.fingerprint()
            }
        );
    }

    #[test]
    fn claiming_someone_elses_fingerprint_without_their_key_is_refused() {
        // The hijack this protocol exists to stop: the victim's certificate and
        // fingerprint are public, so present them — and sign with your own key.
        let victim = identity();
        let attacker = identity();
        let mut hello = greeting(&attacker, "n1");
        hello.fingerprint = victim.fingerprint();
        hello.certificate = victim.cert_der().to_vec();

        assert_eq!(code_of(admit("n1", &hello)), "proof");
    }

    #[test]
    fn a_proof_recorded_from_another_connection_is_refused() {
        let identity = identity();
        assert_eq!(
            code_of(admit("this-nonce", &greeting(&identity, "an-old-nonce"))),
            "proof"
        );
    }

    #[test]
    fn a_wrong_protocol_version_is_refused_with_both_versions_named() {
        let mut hello = greeting(&identity(), "n1");
        hello.protocol_version = PROTOCOL_VERSION + 1;

        let Admission::Reject { code, message } = admit("n1", &hello) else {
            panic!("expected a refusal");
        };
        assert_eq!(code, "protocol_version");
        assert!(message.contains(&PROTOCOL_VERSION.to_string()));
        assert!(message.contains(&(PROTOCOL_VERSION + 1).to_string()));
    }

    #[test]
    fn a_tunnel_without_a_fingerprint_is_refused() {
        let mut hello = greeting(&identity(), "n1");
        hello.fingerprint = "  ".to_string();
        assert_eq!(code_of(admit("n1", &hello)), "fingerprint");
    }

    #[test]
    fn a_pair_offer_is_filed_under_the_tunnels_own_fingerprint() {
        let cloud = Cloud::new();
        let now = now_secs();
        let offer = TunnelFrame {
            kind: Some(tunnel_frame::Kind::PairOffer(PairOffer {
                code_hash: "hash".to_string(),
                expires_at: now + 300,
            })),
        };

        assert!(handle(&cloud, "desk-a", offer, now).is_none());
        assert_eq!(cloud.offers().route("hash", now).as_deref(), Some("desk-a"));
    }

    #[test]
    fn a_ping_is_answered() {
        let cloud = Cloud::new();
        let reply = handle(
            &cloud,
            "desk-a",
            TunnelFrame {
                kind: Some(tunnel_frame::Kind::Ping(Default::default())),
            },
            0,
        );
        assert!(matches!(
            reply.and_then(|frame| frame.kind),
            Some(tunnel_frame::Kind::Pong(_))
        ));
    }

    // -- over a real socket --------------------------------------------------

    pub(crate) type Client = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    pub(crate) async fn serve(cloud: Arc<Cloud>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binds");
        let address = listener.local_addr().expect("an address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router(cloud)).await;
        });
        format!("ws://{address}/tunnel")
    }

    pub(crate) async fn next(client: &mut Client) -> TunnelFrame {
        loop {
            let message = tokio::time::timeout(Duration::from_secs(5), client.next())
                .await
                .expect("a frame in time")
                .expect("open")
                .expect("no error");
            if let Ok(text) = message.to_text() {
                if !text.is_empty() {
                    return decode(text).expect("a frame");
                }
            }
        }
    }

    pub(crate) async fn say(client: &mut Client, frame: &TunnelFrame) {
        let text = encode(frame).expect("encodes");
        client
            .send(tokio_tungstenite::tungstenite::Message::text(text))
            .await
            .expect("sends");
    }

    /// Connect as `identity`, answer the challenge, and return the open socket.
    pub(crate) async fn connect_as(url: &str, identity: &Identity) -> (Client, TunnelFrame) {
        let (mut client, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("connects");
        let Some(tunnel_frame::Kind::Challenge(challenge)) = next(&mut client).await.kind else {
            panic!("the cloud must challenge first");
        };
        say(
            &mut client,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Hello(greeting(
                    identity,
                    &challenge.nonce,
                ))),
            },
        )
        .await;
        let verdict = next(&mut client).await;
        (client, verdict)
    }

    async fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(tokio::time::Instant::now() < deadline, "timed out waiting");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn a_desktop_that_answers_the_challenge_is_attached() {
        let cloud = Arc::new(Cloud::new());
        let url = serve(Arc::clone(&cloud)).await;
        let identity = identity();

        let (_client, verdict) = connect_as(&url, &identity).await;

        assert!(matches!(verdict.kind, Some(tunnel_frame::Kind::Welcome(_))));
        let fingerprint = identity.fingerprint();
        wait_until(|| cloud.hub().is_online(&fingerprint)).await;
    }

    #[tokio::test]
    async fn an_impostor_is_refused_and_does_not_displace_the_real_desktop() {
        let cloud = Arc::new(Cloud::new());
        let url = serve(Arc::clone(&cloud)).await;
        let real = identity();
        let fingerprint = real.fingerprint();

        let (mut real_client, _) = connect_as(&url, &real).await;
        wait_until(|| cloud.hub().is_online(&fingerprint)).await;

        // The impostor knows the fingerprint and the certificate — both public —
        // but signs with its own key.
        let (mut impostor, _) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connects");
        let Some(tunnel_frame::Kind::Challenge(challenge)) = next(&mut impostor).await.kind else {
            panic!("a challenge");
        };
        let mut forged = greeting(&identity(), &challenge.nonce);
        forged.fingerprint = fingerprint.clone();
        forged.certificate = real.cert_der().to_vec();
        say(
            &mut impostor,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Hello(forged)),
            },
        )
        .await;

        let verdict = next(&mut impostor).await;
        let Some(tunnel_frame::Kind::Rejected(refusal)) = verdict.kind else {
            panic!("expected a refusal, got {verdict:?}");
        };
        assert_eq!(refusal.code, "proof");

        // And the real desktop's tunnel is untouched: a request still reaches it.
        let hub_cloud = Arc::clone(&cloud);
        let print = fingerprint.clone();
        let asked = tokio::spawn(async move {
            hub_cloud
                .hub()
                .request(&print, "GET", "/api/status", String::new(), "t")
                .await
        });
        let Some(tunnel_frame::Kind::Request(request)) = next(&mut real_client).await.kind else {
            panic!("the real desktop still receives its requests");
        };
        say(
            &mut real_client,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Response(
                    flexdeck_protocol::TunnelResponse {
                        id: request.id,
                        status: 200,
                        body: "{}".to_string(),
                    },
                )),
            },
        )
        .await;
        assert_eq!(asked.await.expect("joins").expect("answered").status, 200);
    }

    #[tokio::test]
    async fn a_reconnect_is_not_undone_by_the_old_connection_ending() {
        let cloud = Arc::new(Cloud::new());
        let url = serve(Arc::clone(&cloud)).await;
        let identity = identity();
        let fingerprint = identity.fingerprint();

        let (old, _) = connect_as(&url, &identity).await;
        let (_new, verdict) = connect_as(&url, &identity).await;
        assert!(matches!(verdict.kind, Some(tunnel_frame::Kind::Welcome(_))));

        drop(old);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            cloud.hub().is_online(&fingerprint),
            "the old socket's clean-up must not detach the new tunnel"
        );
    }

    #[tokio::test]
    async fn a_connection_that_never_says_hello_is_refused() {
        let cloud = Arc::new(Cloud::new());
        let url = serve(Arc::clone(&cloud)).await;
        let (mut client, _) = tokio_tungstenite::connect_async(&url)
            .await
            .expect("connects");
        let _challenge = next(&mut client).await;

        // Anything other than a hello in answer.
        say(
            &mut client,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::Ping(Default::default())),
            },
        )
        .await;
        let Some(tunnel_frame::Kind::Rejected(refusal)) = next(&mut client).await.kind else {
            panic!("a refusal");
        };
        assert_eq!(refusal.code, "hello_expected");
    }

    #[tokio::test]
    async fn a_departing_tunnel_takes_its_pairing_offer_with_it() {
        let cloud = Arc::new(Cloud::new());
        let url = serve(Arc::clone(&cloud)).await;
        let identity = identity();

        let (mut client, _) = connect_as(&url, &identity).await;
        say(
            &mut client,
            &TunnelFrame {
                kind: Some(tunnel_frame::Kind::PairOffer(PairOffer {
                    code_hash: "hash".to_string(),
                    expires_at: now_secs() + 300,
                })),
            },
        )
        .await;
        wait_until(|| !cloud.offers().is_empty()).await;

        drop(client);
        wait_until(|| cloud.offers().is_empty()).await;
    }
}
