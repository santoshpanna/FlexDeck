//! The flexdeck agent: a command-line host for the deck.
//!
//! Everything that matters — opening the store, loading the identity, applying
//! settings, installing the executor, building the router — lives in this crate's
//! library, because the desktop shell needs the same sequence and a second copy
//! of it would drift. What is left here is argument parsing and the console
//! experience.
//!
//! The order still matters, which is why it is not spelled out again below: see
//! the [`Host::assemble`] documentation for what happens and in what order.

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use flexdeck_agent::{Host, Options, DEFAULT_DATA_DIR};
use flexdeck_pairing::session::DEFAULT_TTL_SECS;

#[tokio::main]
async fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(String::as_str) == Some("verify-plugin") {
        // Run a plugin the way this agent would, and say whether it will work.
        return ExitCode::from(flexdeck_server::verify::cli(&raw[1..]) as u8);
    }
    let args = match Args::parse(raw.into_iter()) {
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
            tracing::error!("the agent could not start: {error}");
            eprintln!("the agent could not start: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Err(error) = flexdeck_agent::logging::init(&args.data_dir, flexdeck_agent::logging::DEFAULT_LEVEL) {
        eprintln!("warning: could not open the log file: {error}");
    }
    let host = Host::assemble(Options {
        data_dir: args.data_dir,
        port: args.port,
        bind: args.bind,
        cloud: args.cloud,
    })?;

    host.report();

    if args.show_admin_token {
        match host.agent().admin_token() {
            Ok(token) => {
                println!("  admin token  {token}");
                println!("               (debugging only; do not share it)");
            }
            Err(error) => println!("  admin token  <unavailable: {error}>"),
        }
    }

    // The console is still the quickest way to pair a tablet on a machine with no
    // window open. The shell shows the same PIN on demand instead.
    let offer = host.agent().begin_pairing_now();
    println!("  pairing PIN  {}", offer.pin);
    println!("               (valid for {DEFAULT_TTL_SECS}s; a QR is available from the API)");

    host.serve().await
}

#[derive(Debug)]
struct Args {
    data_dir: PathBuf,
    port: Option<u16>,
    bind: Option<IpAddr>,
    cloud: Option<String>,
    show_admin_token: bool,
}

impl Args {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut args = Self {
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
            port: None,
            bind: None,
            cloud: None,
            show_admin_token: false,
        };

        let mut arguments = arguments;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--help" | "-h" => return Ok(None),
                "--data-dir" => {
                    let value = arguments.next().ok_or("--data-dir needs a path")?;
                    args.data_dir = PathBuf::from(value);
                }
                "--port" => {
                    let value = arguments.next().ok_or("--port needs a number")?;
                    args.port = Some(
                        value
                            .parse()
                            .map_err(|_| format!("'{value}' is not a port number"))?,
                    );
                }
                "--bind" => {
                    let value = arguments.next().ok_or("--bind needs an IP address")?;
                    // A name is refused rather than resolved: picking one address
                    // for a name is exactly the kind of guessing that produces a
                    // QR nobody can use.
                    args.bind = Some(value.parse().map_err(|_| {
                        format!("'{value}' is not an IP address (host names are not resolved here)")
                    })?);
                }
                "--cloud" => {
                    let value = arguments.next().ok_or("--cloud needs a URL")?;

                    // Checked here, at the moment the user typed it, rather than
                    // surfacing later as a tunnel that quietly never connects.
                    flexdeck_server::net::check_portal_url(&value)?;

                    args.cloud = Some(value);
                }
                "--show-admin-token" => args.show_admin_token = true,
                other => return Err(format!("unrecognised argument '{other}'")),
            }
        }

        Ok(Some(args))
    }

    fn print_help() {
        println!("flexdeck — a local-network control surface for old tablets");
        println!();
        println!("Usage: flexdeck [OPTIONS]");
        println!("       flexdeck verify-plugin <folder | .fdpkg> [--run-actions] [--json] [--out <file>]");
        println!();
        println!("Options:");
        println!("  --data-dir <path>   where to keep the database and identity");
        println!("                      (default: {DEFAULT_DATA_DIR})");
        println!("  --port <number>     port to listen on and advertise");
        println!("  --bind <ip>         interface to accept connections on");
        println!("  --cloud <url>       portal to reach this desktop from, e.g.");
        println!("                      wss://portal.example.com/tunnel");
        println!("  --show-admin-token  print the admin credential (debugging only;");
        println!("                      the portal does not need a human to hold it)");
        println!("  -h, --help          show this message");
        println!();
        println!("Settings are stored in the database, so options given here override");
        println!("them for one run only. Port and bind address are not persisted.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Option<Args>, String> {
        Args::parse(arguments.iter().map(|value| value.to_string()))
    }

    #[test]
    fn no_arguments_means_defaults() {
        let args = parse(&[]).expect("parses").expect("not a help request");

        assert_eq!(args.data_dir, PathBuf::from(DEFAULT_DATA_DIR));
        assert_eq!(args.port, None, "unset means 'use the stored setting'");
        assert_eq!(args.bind, None);
        assert_eq!(
            args.cloud, None,
            "no portal means web access is not offered"
        );
        assert!(
            !args.show_admin_token,
            "the admin token is not printed unless asked for"
        );
    }

    #[test]
    fn a_cloud_url_is_read() {
        let args = parse(&["--cloud", "wss://portal.example.com/tunnel"])
            .expect("parses")
            .expect("not a help request");

        assert_eq!(
            args.cloud.as_deref(),
            Some("wss://portal.example.com/tunnel")
        );
    }

    #[test]
    fn a_cloud_url_that_is_not_a_websocket_url_is_refused_at_the_prompt() {
        // Checked where the user typed it, rather than surfacing later as a
        // tunnel that quietly never connects.
        for wrong in ["https://portal.example.com", "portal.example.com", ""] {
            let outcome = parse(&["--cloud", wrong]);
            assert!(outcome.is_err(), "'{wrong}' should have been refused");
        }
    }

    #[test]
    fn the_admin_token_is_only_shown_when_asked_for() {
        let args = parse(&["--show-admin-token"])
            .expect("parses")
            .expect("not a help request");

        assert!(args.show_admin_token);
    }

    #[test]
    fn overrides_are_read() {
        let args = parse(&[
            "--data-dir",
            "/tmp/deck",
            "--port",
            "9000",
            "--bind",
            "0.0.0.0",
        ])
        .expect("parses")
        .expect("not a help request");

        assert_eq!(args.data_dir, PathBuf::from("/tmp/deck"));
        assert_eq!(args.port, Some(9000));
        assert_eq!(args.bind, Some("0.0.0.0".parse().expect("an address")));
    }

    #[test]
    fn help_is_a_request_not_an_error() {
        assert!(parse(&["--help"]).expect("parses").is_none());
        assert!(parse(&["-h"]).expect("parses").is_none());
    }

    #[test]
    fn an_unknown_argument_is_rejected_rather_than_ignored() {
        // Silently ignoring a mistyped flag would start the agent with settings
        // the user did not ask for, which is worse than refusing.
        let error = parse(&["--prot", "9000"]).expect_err("should reject");
        assert!(error.contains("--prot"), "{error}");
    }

    #[test]
    fn a_flag_without_its_value_is_rejected() {
        assert!(parse(&["--port"]).is_err());
        assert!(parse(&["--data-dir"]).is_err());
    }

    #[test]
    fn a_malformed_value_is_rejected() {
        assert!(parse(&["--port", "not-a-number"]).is_err());
        assert!(parse(&["--port", "70000"]).is_err(), "out of range");
        assert!(parse(&["--bind", "192.168.1.999"]).is_err());
    }

    #[test]
    fn a_hostname_is_refused_for_binding() {
        let error = parse(&["--bind", "deckpad.local"]).expect_err("should reject");
        assert!(error.contains("not resolved"), "{error}");
    }
}
