//! Assembly for the flexdeck agent.
//!
//! The command-line binary and the desktop shell need the *same* start-up
//! sequence, and it is a sequence where order matters: the identity is **loaded**
//! if present rather than regenerated, settings are validated before anything
//! binds, the input executor is installed before anything reports on whether
//! presses will work, and the listener is built last.
//!
//! Duplicating that in a second binary is how the two drift, and the failure it
//! would produce is silent: a regenerated identity, after which every paired
//! device rejects the agent while the agent itself looks perfectly healthy. So
//! the sequence lives here once. What a caller then does with a [`Host`] — print
//! a banner, open a window, serve headless — is the caller's business.
//!
//! The banner is printed by [`Host::report`] rather than by each caller, because
//! the two entry points want the same words and only one of them is a console
//! program by nature.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use flexdeck_actions::EnigoExecutor;
use flexdeck_pairing::Identity;
use flexdeck_server::tunnel::TunnelConfig;
use flexdeck_server::web_access::{self, WebAccess};
use flexdeck_server::{http, loopback, Agent, Settings, StartupWarning};
use flexdeck_store::ConfigStore;

/// Where the agent keeps its database and identity unless told otherwise.
pub const DEFAULT_DATA_DIR: &str = ".flexdeck";

/// Anything assembly can go wrong with.
pub type HostResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// What the caller asked for on the command line.
#[derive(Debug)]
pub struct Options {
    pub data_dir: PathBuf,
    /// Overrides the stored port for this run only.
    pub port: Option<u16>,
    /// Overrides the stored bind address for this run only.
    pub bind: Option<IpAddr>,
    /// A portal to reach this desktop from. Persisted, unlike the two above.
    pub cloud: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from(DEFAULT_DATA_DIR),
            port: None,
            bind: None,
            cloud: None,
        }
    }
}

/// An assembled agent, ready to serve or to be driven by a shell.
pub struct Host {
    agent: Arc<Agent>,
    settings: Settings,
    bind: SocketAddr,
    web_access: Option<Arc<WebAccess>>,
    seeded: bool,
}

impl Host {
    /// Open the store and identity, apply overrides, install the executor, and
    /// prepare the tunnel.
    ///
    /// Does not serve. Callers that want the server running call [`Host::serve`];
    /// a shell keeps the [`Host::agent`] handle instead and drives it in-process.
    ///
    /// Must be called from within a Tokio runtime: enabling web access spawns the
    /// tunnel task.
    pub fn assemble(options: Options) -> HostResult<Self> {
        std::fs::create_dir_all(&options.data_dir)?;

        let store = ConfigStore::open(options.data_dir.join("flexdeck.db"))?;
        let identity = Identity::load_or_generate(
            options.data_dir.join("identity"),
            vec!["localhost".to_string()],
        )?;

        let mut settings = Settings::load(&store)?;
        if let Some(port) = options.port {
            settings.port = port;
        }
        if let Some(bind) = options.bind {
            settings.bind_address = bind;
        }
        if let Some(cloud) = options.cloud.clone() {
            // Persisted, not per-run: the portal is where this installation
            // lives, not a one-off choice, and a value that vanished on restart
            // would silently stop the tunnel from ever coming back.
            settings.cloud_url = Some(cloud);
            settings.save(&store)?;
        }
        settings.validate()?;

        let agent = Arc::new(Agent::new(
            store,
            identity,
            settings.advertised_host(),
            settings.port,
        ));

        let seeded = agent.seed_default_config()?;

        // Installed *before* anything reports, so the warnings describe the agent
        // as it will actually behave rather than as it is right now.
        match EnigoExecutor::new() {
            Ok(executor) => agent.install_live_executor(Box::new(executor)),
            Err(error) => {
                // Not fatal: the configuration, the portal and pairing all still
                // work. Only pressing a button does nothing.
                eprintln!("warning: no input system is available, so actions will do nothing");
                eprintln!("         ({error})");
            }
        }

        // Real volume, real notifications, real launching. Same reasoning as
        // the executor: installed here, once, so a test that builds an `Agent`
        // directly can never change the machine's volume.
        agent.install_live_backends(
            flexdeck_plugin::builtin::Backends::live(),
            Box::new(flexdeck_plugin::builtin::notifications::DesktopNotifier),
        );
        // A plugin's sign-in page (Spotify, Discord) opens in the real browser.
        // The host has already refused anything that is not http(s).
        agent.install_url_opener(Box::new(|url| {
            open::that_detached(url).map_err(|error| error.to_string())
        }));

        // Installed plugins: started now, so a deck that uses one works from
        // the first press, and supervised for the agent's lifetime. A plugin
        // that fails to start is reported, never fatal.
        agent.enable_media(&options.data_dir.join("media"))?;
        let plugins = agent.enable_plugins(
            options.data_dir.join("plugins"),
            flexdeck_plugin_host::Runtimes::discover(),
            flexdeck_plugin_host::Options::default(),
        );

        // The authoring loop: `FLEXDECK_DEV_PLUGINS=<dir>[;<dir>…]` runs each
        // folder as a linked plugin, reloaded on every save.
        if let Some(paths) = std::env::var_os("FLEXDECK_DEV_PLUGINS") {
            for path in std::env::split_paths(&paths) {
                match plugins.link_dev(&path) {
                    Ok(linked) => {
                        eprintln!("linked plugin '{}' from {}", linked.id, path.display())
                    }
                    Err(error) => eprintln!("warning: could not link {}: {error}", path.display()),
                }
            }
        }

        // The API, plus the toggle — but only where there is a portal to toggle.
        // An agent with no portal configured keeps the smaller surface: this
        // binary does not OFFER web access rather than offering it and then
        // failing. The desktop WINDOW always has one, because the window is where
        // an address can be typed.
        let web_access = match settings.cloud_url.clone() {
            Some(url) => {
                let access = Arc::new(WebAccess::new(
                    Arc::clone(&agent),
                    http::router(Arc::clone(&agent)),
                    TunnelConfig {
                        url: Some(url),
                        name: desktop_name(),
                    },
                ));

                // Resume rather than reset: the user turned web access on, and a
                // restart is not a decision to turn it off.
                if access.is_enabled()? {
                    access.enable()?;
                }

                Some(access)
            }
            None => None,
        };

        // The reactive engine's variable-change subscription. Spawning it here
        // means a plugin's `refresh` call actually fires variable-triggered
        // automations on a running agent, not just in tests. The task lives
        // until the receiver closes, which happens at shutdown.
        agent.behavior().start(&tokio::runtime::Handle::current());

        let bind = settings.bind();

        Ok(Self {
            agent,
            settings,
            bind,
            web_access,
            seeded,
        })
    }

    /// The running agent, for a caller that wants to drive it directly.
    pub fn agent(&self) -> Arc<Agent> {
        Arc::clone(&self.agent)
    }

    /// `None` when this installation has no portal configured.
    pub fn web_access(&self) -> Option<Arc<WebAccess>> {
        self.web_access.clone()
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn bind(&self) -> SocketAddr {
        self.bind
    }

    /// The address and port clients should be told, which is not always the one
    /// this process is listening on.
    pub fn advertised(&self) -> (String, u16) {
        (self.settings.advertised_host(), self.settings.port)
    }

    /// The failures that look fine from this machine but not from a tablet.
    pub fn warnings(&self) -> Vec<StartupWarning> {
        self.agent.startup_warnings(self.bind)
    }

    /// Whether this run created the placeholder configuration.
    pub fn seeded_placeholder(&self) -> bool {
        self.seeded
    }

    /// The router, with the web-access toggle merged in where there is one.
    ///
    /// Exposed so a caller that hosts its own server can nest this one instead of
    /// calling [`Host::serve`].
    pub fn router(&self) -> axum::Router {
        router_for(&self.agent, self.web_access.as_ref())
    }

    /// The same state, taken apart.
    ///
    /// A desktop shell needs to *hold* the agent — to read its state while
    /// serving requests from it — and [`Host::serve`] consumes the host. Taking
    /// the parts out is how one caller keeps the handles and another serves.
    pub fn into_parts(self) -> HostParts {
        HostParts {
            agent: self.agent,
            settings: self.settings,
            bind: self.bind,
            web_access: self.web_access,
        }
    }

    /// Print what the user needs to know.
    ///
    /// Deliberately does **not** open a pairing window: that is a side effect a
    /// caller chooses, and the desktop shell shows a PIN on demand instead.
    pub fn report(&self) {
        if self.seeded {
            println!("No configuration was found, so a placeholder one was created.");
            println!("Replace it once you have a window open.");
        }

        println!();
        println!("flexdeck agent");
        println!("  fingerprint  {}", self.agent.identity().fingerprint());
        println!(
            "  address      {}:{}",
            self.settings.advertised_host(),
            self.settings.port
        );

        if self.settings.host_is_overridden() {
            println!("  (that address is an override, not a discovered one)");
        }

        if let (Some(access), Some(portal)) = (&self.web_access, self.settings.cloud_url.as_ref()) {
            // An unreadable preference is reported as "off" rather than guessed
            // at: claiming it is on when the store disagrees would be the
            // comforting lie this line exists to avoid.
            let enabled = access.is_enabled().unwrap_or(false);

            // "Enabled and not running" is a real state — the portal is simply
            // unreachable — and saying so is the honest answer rather than a
            // comforting one.
            let state = match (enabled, access.is_running()) {
                (true, true) => format!("on, connected to {portal}"),
                (true, false) => format!("on, NOT connected to {portal}"),
                (false, _) => "off".to_string(),
            };
            println!("  web access   {state}");
        }

        // The admin token is deliberately not printed. The portal reaches the
        // admin routes *through the tunnel*, where the agent attaches its own
        // credential, so no human needs to hold it — and printing it put a master
        // key on screen beside a window that presents itself as the user's
        // interface. `--show-admin-token` remains for debugging.
        for warning in self.warnings() {
            println!("  warning      {}", warning.message());
        }
    }

    /// Bind and serve until the process ends.
    pub async fn serve(self) -> HostResult<()> {
        serve_parts(self.into_parts()).await
    }
}

/// An assembled agent in pieces, for a caller that keeps the handles.
pub struct HostParts {
    pub agent: Arc<Agent>,
    pub settings: Settings,
    pub bind: SocketAddr,
    pub web_access: Option<Arc<WebAccess>>,
}

/// The router for an assembled agent, with the toggle merged in where there is one.
pub fn router_for(agent: &Arc<Agent>, web_access: Option<&Arc<WebAccess>>) -> axum::Router {
    match web_access {
        Some(access) => {
            http::router(Arc::clone(agent)).merge(web_access::router(Arc::clone(access)))
        }
        None => http::router(Arc::clone(agent)),
    }
}

/// Bind and serve until the process ends.
///
/// Free-standing so a shell can serve from the parts it kept, rather than having
/// to give up the agent to start the server.
pub async fn serve_parts(parts: HostParts) -> HostResult<()> {
    let app = router_for(&parts.agent, parts.web_access.as_ref());

    // TLS is not optional. Every API call carries a bearer token in a header, and
    // over plain HTTP that token is readable by anything on the network, which
    // would undo the authentication entirely.
    let tls = flexdeck_server::tls::rustls_config(parts.agent.identity()).await?;

    // Bound explicitly, before anything claims the server is up. `bind_rustls`
    // defers the bind to `serve`, so a taken port used to be announced as
    // "Serving on …" and refused only afterwards — which reads as a server that
    // started and then died rather than one that never started at all.
    let listener = std::net::TcpListener::bind(parts.bind)?;
    listener.set_nonblocking(true)?;
    let server = axum_server::from_tcp_rustls(listener, tls)?;

    println!();
    println!("Serving on https://{}", parts.bind);
    println!("               (the certificate is self-signed; a browser will warn once)");

    serve_loopback(&parts, app.clone()).await;

    server.serve(app.into_make_service()).await?;
    Ok(())
}

/// The port the same-PC door listens on: the agent's own, plus one.
pub fn loopback_port(settings: &Settings) -> u16 {
    settings.port.wrapping_add(1)
}

/// Open the same-PC door (see `flexdeck_server::loopback`) beside the LAN
/// listener.
///
/// Plain HTTP on 127.0.0.1 only — nothing leaves the machine, and a browser
/// treats `http://127.0.0.1` as a secure context. Not fatal if the port is
/// taken: the LAN listener and the cloud still work, and the portal on this
/// computer can still pair with a code.
async fn serve_loopback(parts: &HostParts, api: axum::Router) {
    let port = loopback_port(&parts.settings);
    let config = loopback::LoopbackConfig {
        port,
        allowed_origins: loopback::allowed_origins(parts.settings.cloud_url.as_deref()),
        name: desktop_name(),
    };
    let app = loopback::router(Arc::clone(&parts.agent), api, config);

    match tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await {
        Ok(listener) => {
            println!("Serving this computer's browser on http://127.0.0.1:{port}");
            tokio::spawn(async move {
                if let Err(error) = axum::serve(listener, app).await {
                    eprintln!("warning: the same-PC listener stopped: {error}");
                }
            });
        }
        Err(error) => {
            eprintln!(
                "warning: could not open http://127.0.0.1:{port} for this computer's browser \
                 ({error}); pair it with a code instead"
            );
        }
    }
}

/// A label for this desktop, as the website will show it.
///
/// Best effort, and deliberately never empty: a name an owner recognises beats a
/// fingerprint, but an empty string in a list of desktops is worse than either.
pub fn desktop_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "This desktop".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_name_is_never_empty() {
        // An empty label in a list of desktops is worse than a generic one.
        assert!(!desktop_name().trim().is_empty());
    }

    #[test]
    fn the_default_options_are_the_documented_ones() {
        let options = Options::default();

        assert_eq!(options.data_dir, PathBuf::from(DEFAULT_DATA_DIR));
        assert!(
            options.port.is_none(),
            "unset means 'use the stored setting'"
        );
        assert!(options.bind.is_none());
        assert!(options.cloud.is_none(), "no portal is not offered");
    }
}
