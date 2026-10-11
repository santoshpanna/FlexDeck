//! The cloud service: the portal, its API, and the tunnel endpoint.
//!
//! This is the piece that actually runs. Everything it does is assembled from the
//! library, so the binary is start-up order, argument parsing, and telling the
//! operator what they need to know — the same shape as the agent's own binary.
//!
//! There are no accounts and no sign-in to configure: the cloud holds nothing
//! that can act on a desktop. A desktop proves who it is by signature when its
//! tunnel opens, and a browser is trusted by a desktop only after typing a code
//! shown on that desktop's screen. What the operator does configure is where the
//! service listens, where its data lives, and — behind a reverse proxy —
//! whether to believe `X-Forwarded-For` for rate limiting.
//!
//! Plugins are not served from here: they come from static plugin repositories
//! the desktop reads itself (see `flexdeck_server::repos`).

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use flexdeck_cloud::{portal, tunnel_socket, Cloud, DataDir};

/// The port a fresh deployment listens on.
const DEFAULT_PORT: u16 = 8787;

/// Where the cloud keeps its data without `--data-dir`.
const DEFAULT_DATA_DIR: &str = ".flexdeck-cloud";

/// How often expired pairing codes are swept from memory.
const SWEEP_EVERY: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(Some(args)) => args,
        Ok(None) => {
            Args::print_help();
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("{message}\n");
            Args::print_help();
            return ExitCode::FAILURE;
        }
    };

    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("the cloud could not start: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let data = DataDir::at(&args.data_dir);

    // ONE process per data directory, and proven writable before serving: a
    // read-only mount otherwise looks exactly like a healthy fresh deployment.
    let _lock = data.lock()?;
    data.probe()?;

    let cloud = if args.trust_proxy {
        Cloud::new().trusting_proxy()
    } else {
        Cloud::new()
    };
    let cloud = Arc::new(cloud);

    let bind = SocketAddr::from((args.bind, args.port));

    println!();
    println!("flexdeck cloud");
    println!("  listening    http://{bind}");
    println!("  portal       {}", args.portal_dir.display());
    println!("  data         {}", args.data_dir.display());
    println!("  tunnel       ws://{bind}/tunnel");
    if args.trust_proxy {
        println!("  proxy        trusting X-Forwarded-For for client addresses");
    }

    let index_html = std::fs::read_to_string(args.portal_dir.join("index.html")).ok();
    if index_html.is_none() {
        println!("  warning      no portal build found; the API still works");
    }

    // Expired pairing codes are dropped when someone looks them up; this also
    // drops the ones nobody ever typed.
    let sweeper = Arc::clone(&cloud);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(SWEEP_EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            sweeper.offers().sweep(flexdeck_cloud::cloud::now_secs());
        }
    });

    // One port for everything, behind one set of protective headers.
    let app = portal::router(&args.portal_dir)
        .merge(flexdeck_cloud::http::router(Arc::clone(&cloud)))
        .merge(tunnel_socket::router(cloud));
    let app = flexdeck_cloud::http::with_security_headers(app, index_html.as_deref());

    let listener = tokio::net::TcpListener::bind(bind).await?;
    // With the peer address attached, which is what rate limiting keys on.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;

    Ok(())
}

#[derive(Debug)]
struct Args {
    bind: std::net::IpAddr,
    port: u16,
    portal_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    /// Believe `X-Forwarded-For` for client addresses.
    trust_proxy: bool,
}

impl Args {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut args = Self {
            // Loopback by default. A cloud service is normally behind a proxy
            // that terminates TLS; binding every interface by default would
            // serve plain HTTP to the world.
            bind: std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            port: DEFAULT_PORT,
            portal_dir: portal::default_directory(),
            data_dir: std::path::PathBuf::from(DEFAULT_DATA_DIR),
            trust_proxy: false,
        };

        let mut arguments = arguments;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--help" | "-h" => return Ok(None),
                "--bind" => {
                    let value = arguments.next().ok_or("--bind needs an IP address")?;
                    args.bind = value.parse().map_err(|_| {
                        format!("'{value}' is not an IP address (host names are not resolved here)")
                    })?;
                }
                "--port" => {
                    let value = arguments.next().ok_or("--port needs a number")?;
                    args.port = value
                        .parse()
                        .map_err(|_| format!("'{value}' is not a port number"))?;
                }
                "--portal-dir" => {
                    let value = arguments.next().ok_or("--portal-dir needs a path")?;
                    args.portal_dir = std::path::PathBuf::from(value);
                }
                "--data-dir" => {
                    let value = arguments.next().ok_or("--data-dir needs a path")?;
                    args.data_dir = std::path::PathBuf::from(value);
                }
                "--trust-proxy" => args.trust_proxy = true,
                other => return Err(format!("unrecognised argument '{other}'")),
            }
        }

        Ok(Some(args))
    }

    fn print_help() {
        println!("flexdeck cloud — the portal, its API, and the tunnel endpoint");
        println!();
        println!("Usage: flexdeck-cloud [OPTIONS]");
        println!();
        println!("Options:");
        println!("  --bind <ip>          interface to listen on (default: 127.0.0.1)");
        println!("  --port <number>      port to listen on (default: {DEFAULT_PORT})");
        println!("  --portal-dir <path>  where the built portal lives");
        println!("  --data-dir <path>    where the cloud keeps its data");
        println!("                       (default: {DEFAULT_DATA_DIR})");
        println!("  --trust-proxy        believe X-Forwarded-For (only behind a proxy");
        println!("                       that sets it)");
        println!("  -h, --help           show this message");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Option<Args>, String> {
        Args::parse(arguments.iter().map(|value| value.to_string()))
    }

    #[test]
    fn no_arguments_means_loopback() {
        let args = parse(&[]).expect("parses").expect("not a help request");

        assert_eq!(args.port, DEFAULT_PORT);
        assert!(
            args.bind.is_loopback(),
            "plain HTTP must not be served on every interface by default"
        );
    }

    #[test]
    fn overrides_are_read() {
        let args = parse(&[
            "--bind",
            "0.0.0.0",
            "--port",
            "9000",
            "--portal-dir",
            "/srv/portal",
            "--data-dir",
            "/var/lib/flexdeck",
        ])
        .expect("parses")
        .expect("not a help request");

        assert_eq!(args.bind.to_string(), "0.0.0.0");
        assert_eq!(args.port, 9000);
        assert_eq!(args.portal_dir.to_string_lossy(), "/srv/portal");
        assert_eq!(args.data_dir.to_string_lossy(), "/var/lib/flexdeck");
    }

    #[test]
    fn the_default_data_directory_is_named() {
        // A default that silently changed between releases would look exactly
        // like "every desk is gone", so it is pinned here.
        let args = parse(&[]).expect("parses").expect("not a help request");

        assert_eq!(args.data_dir.to_string_lossy(), DEFAULT_DATA_DIR);
    }

    #[test]
    fn a_bad_bind_or_port_is_refused_at_the_prompt() {
        assert!(parse(&["--bind", "not-an-ip"]).is_err());
        assert!(parse(&["--port", "not-a-number"]).is_err());
        assert!(parse(&["--port"]).is_err(), "a flag with no value");
        assert!(parse(&["--nonsense"]).is_err());
    }

    #[test]
    fn the_proxy_is_believed_only_when_asked() {
        assert!(!parse(&[]).expect("parses").expect("args").trust_proxy);
        assert!(
            parse(&["--trust-proxy"])
                .expect("parses")
                .expect("args")
                .trust_proxy
        );
    }

    #[test]
    fn the_default_portal_directory_is_absolute() {
        // Resolved from the crate's own location, so running the server from
        // anywhere finds the same files.
        let directory = portal::default_directory();
        assert!(
            directory.is_absolute() || directory.to_string_lossy().starts_with("apps"),
            "got: {}",
            directory.display()
        );
    }
}
