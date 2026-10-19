//! Installed plugins: install, uninstall, update, enable, reload — while the
//! agent runs.
//!
//! # A package
//!
//! A plugin travels as a `.fdpkg`: a zip with `plugin.json` at its root (see
//! [`abi::PackageManifest`]) and whatever files its entry point needs. Its
//! identity is its SHA-256, which a repository's index pins and the installer
//! checks, so what was listed is what runs (see [`crate::repos`]).
//!
//! # On disk
//!
//! ```text
//! <data>/plugins/<id>/<version>-<sha8>/   one directory per installed build
//! ```
//!
//! A new build is unpacked beside the old one and *started* before anything is
//! swapped. Only once it has answered `initialize` with an acceptable manifest
//! does it replace the old build in the registry; if it fails, the old build
//! keeps running and the new directory is removed. That is what makes an
//! update safe to press on a deck that is in use.
//!
//! # Hot reload
//!
//! Nothing here restarts the agent. Registering, replacing and unregistering
//! all go through the [`Registry`], which already treats a replaced plugin as
//! "shut the old one down", and every change bumps the catalogue generation so
//! open editors refresh.
//!
//! A *linked* plugin (`link_dev`) runs straight from a working directory and is
//! restarted whenever a file in it changes — the authoring loop.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use flexdeck_plugin::{abi, Origin, Registry};
use flexdeck_plugin_host::{HostEvent, Options, Runtimes, Services, SidecarPlugin, SpawnSpec};
use flexdeck_store::ConfigStore;
use flexdeck_vars::VariableStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Metadata key the installed-plugin records live under.
pub const INSTALLED_KEY: &str = "plugins.installed";

/// The largest package accepted, compressed. A deck plugin is a script and an
/// icon; anything near this is almost certainly not one.
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;
/// The largest total size a package may unpack to — the zip-bomb guard.
pub const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
/// The most files a package may contain.
pub const MAX_ENTRIES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum PluginsError {
    #[error("the package is not a readable plugin package: {0}")]
    Package(String),

    #[error(transparent)]
    Abi(#[from] abi::AbiError),

    #[error("the package's checksum is {actual}, not the expected {expected}")]
    Checksum { expected: String, actual: String },

    #[error("this plugin needs permissions that were not accepted: {missing:?}")]
    Permissions {
        missing: Vec<String>,
        package: Box<abi::PackageManifest>,
        sha256: String,
    },

    #[error("'{0}' is a built-in plugin and cannot be replaced")]
    BuiltIn(String),

    #[error("no plugin '{0}' is installed")]
    NotInstalled(String),

    #[error(transparent)]
    Host(#[from] flexdeck_plugin_host::HostError),

    #[error("could not write the plugin to disk: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Store(#[from] flexdeck_store::StoreError),

    #[error("could not download the plugin: {0}")]
    Download(String),

    #[error("{0}")]
    Repository(String),
}

/// Where an installed plugin came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Source {
    /// Uploaded as a file.
    File,
    /// Downloaded from a URL.
    Url { url: String },
    /// Installed from a plugin repository, which is also where its updates
    /// come from.
    Repository { url: String, name: String },
    /// Installed from the old cloud marketplace. Kept so those records still
    /// load; nothing installs this way any more.
    Marketplace,
    /// Linked from a working directory, and reloaded when it changes.
    Dev { path: PathBuf },
}

/// One installed plugin, as remembered across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub sha256: String,
    /// What the person agreed to. Compared again at every load, so a package
    /// cannot grow a permission after it was installed.
    pub permissions: Vec<String>,
    pub enabled: bool,
    pub source: Source,
    /// The directory it runs from.
    pub path: PathBuf,
    pub installed_at: u64,
}

/// What a package says about itself, before anything is installed — the
/// consent screen's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspected {
    pub package: abi::PackageManifest,
    pub sha256: String,
    pub size: usize,
    /// Set when this id is installed already.
    pub installed_version: Option<String>,
}

/// The installed plugins and everything needed to run them.
pub struct PluginManager {
    root: PathBuf,
    registry: Arc<Mutex<Registry>>,
    vars: Arc<VariableStore>,
    store: Arc<Mutex<ConfigStore>>,
    runtimes: Runtimes,
    options: Options,
    events: mpsc::Sender<HostEvent>,
    /// Why a plugin could not be loaded, by id. Shown next to it in the UI.
    failures: Mutex<BTreeMap<String, String>>,
    /// Each plugin's recent log lines, for the UI.
    logs: Mutex<BTreeMap<String, Vec<String>>>,
    /// Serialises installs, so two installs of one id cannot interleave.
    busy: Mutex<()>,
    stopping: Arc<AtomicBool>,
    /// Where recipe packages' sources run. Absent: a recipe cannot start.
    recipes: std::sync::OnceLock<Arc<crate::sources::Sources>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The SHA-256 of `bytes`, as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Read `plugin.json` out of a package without unpacking anything else.
pub fn read_package_manifest(bytes: &[u8]) -> Result<abi::PackageManifest, PluginsError> {
    abi::package::read_manifest(bytes).map_err(|error| match error {
        abi::package::PackageError::Abi(error) => PluginsError::Abi(error),
        other => PluginsError::Package(other.to_string()),
    })
}

/// Unpack `bytes` into `into`, refusing anything that would land outside it.
/// Unpack a package's files into `into`, as an install does, for
/// [`crate::verify`].
pub(crate) fn unpack_into(bytes: &[u8], into: &Path) -> Result<(), PluginsError> {
    std::fs::create_dir_all(into)?;
    unpack(bytes, into)
}

fn unpack(bytes: &[u8], into: &Path) -> Result<(), PluginsError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| PluginsError::Package(error.to_string()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(PluginsError::Package(format!(
            "it has {} files; the limit is {MAX_ENTRIES}",
            archive.len()
        )));
    }
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| PluginsError::Package(error.to_string()))?;
        let name = entry.name().to_string();
        let relative = name.trim_end_matches('/');
        if relative.is_empty() {
            continue;
        }
        if !abi::is_contained_path(relative) {
            return Err(PluginsError::Package(format!(
                "'{name}' would be written outside the plugin's directory"
            )));
        }
        let target = into.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        // Count what is actually written, not what the header claims.
        let budget = MAX_UNPACKED_BYTES.saturating_sub(total) + 1;
        let written = std::io::copy(&mut (&mut entry).take(budget), &mut out)?;
        total += written;
        if total > MAX_UNPACKED_BYTES {
            return Err(PluginsError::Package(format!(
                "it unpacks to more than {MAX_UNPACKED_BYTES} bytes"
            )));
        }
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o755))?;
        }
    }
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

impl PluginManager {
    /// A manager for plugins kept under `root`.
    ///
    /// Returns the receiving end of the host's event channel; the caller pumps
    /// it (see [`crate::behavior::Behavior::pump_plugin_events`]).
    pub fn new(
        root: PathBuf,
        registry: Arc<Mutex<Registry>>,
        vars: Arc<VariableStore>,
        store: Arc<Mutex<ConfigStore>>,
        runtimes: Runtimes,
        options: Options,
    ) -> (Self, mpsc::Receiver<HostEvent>) {
        let (events, receiver) = mpsc::channel();
        (
            Self {
                root,
                registry,
                vars,
                store,
                runtimes,
                options,
                events,
                failures: Mutex::new(BTreeMap::new()),
                logs: Mutex::new(BTreeMap::new()),
                busy: Mutex::new(()),
                stopping: Arc::new(AtomicBool::new(false)),
                recipes: std::sync::OnceLock::new(),
            },
            receiver,
        )
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every installed plugin, as remembered.
    pub fn installed(&self) -> Vec<Installed> {
        let store = lock(&self.store);
        store
            .meta(INSTALLED_KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    fn save(&self, records: &[Installed]) -> Result<(), PluginsError> {
        let json = serde_json::to_string(records).expect("records always serialise");
        lock(&self.store).set_meta(INSTALLED_KEY, &json)?;
        Ok(())
    }

    fn upsert(&self, record: Installed) -> Result<(), PluginsError> {
        let mut records = self.installed();
        records.retain(|existing| existing.id != record.id);
        records.push(record);
        records.sort_by(|a, b| a.id.cmp(&b.id));
        self.save(&records)
    }

    /// Why a plugin failed to load, if it did.
    pub fn failure(&self, id: &str) -> Option<String> {
        lock(&self.failures).get(id).cloned()
    }

    /// Installed plugins with their live state, for the UI.
    pub fn installed_json(&self) -> serde_json::Value {
        let registry = lock(&self.registry);
        let failures = lock(&self.failures).clone();
        let plugins: Vec<serde_json::Value> = self
            .installed()
            .into_iter()
            .map(|record| {
                let running = registry.origin(&record.id) == Some(Origin::Installed);
                let status = registry.status(&record.id).flatten();
                serde_json::json!({
                    "id": record.id,
                    "name": record.name,
                    "version": record.version,
                    "sha256": record.sha256,
                    "permissions": record.permissions,
                    "enabled": record.enabled,
                    "source": record.source,
                    "installedAt": record.installed_at,
                    "running": running,
                    "status": status,
                    "error": failures.get(&record.id),
                })
            })
            .collect();
        serde_json::json!({ "plugins": plugins })
    }

    /// What a package is, without installing it.
    pub fn inspect(&self, bytes: &[u8]) -> Result<Inspected, PluginsError> {
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(PluginsError::Package(format!(
                "it is {} bytes; the limit is {MAX_PACKAGE_BYTES}",
                bytes.len()
            )));
        }
        let package = read_package_manifest(bytes)?;
        let installed_version = self
            .installed()
            .into_iter()
            .find(|record| record.id == package.id)
            .map(|record| record.version);
        Ok(Inspected {
            sha256: sha256_hex(bytes),
            size: bytes.len(),
            installed_version,
            package,
        })
    }

    /// Install (or update to) the package in `bytes`.
    ///
    /// `accepted` must cover every permission the package declares: the
    /// caller shows [`Self::inspect`]'s answer to a person first, and passes
    /// back what they agreed to. `expected_sha256`, when given, must match.
    pub fn install(
        &self,
        bytes: &[u8],
        expected_sha256: Option<&str>,
        accepted: &[String],
        source: Source,
    ) -> Result<Installed, PluginsError> {
        let _busy = lock(&self.busy);
        let inspected = self.inspect(bytes)?;
        if let Some(expected) = expected_sha256 {
            if !expected.eq_ignore_ascii_case(&inspected.sha256) {
                return Err(PluginsError::Checksum {
                    expected: expected.to_lowercase(),
                    actual: inspected.sha256,
                });
            }
        }
        let package = inspected.package;
        self.refuse_built_in(&package.id)?;

        let accepted: BTreeSet<&String> = accepted.iter().collect();
        let missing: Vec<String> = package
            .permissions
            .iter()
            .filter(|permission| !accepted.contains(permission))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(PluginsError::Permissions {
                missing,
                package: Box::new(package),
                sha256: inspected.sha256,
            });
        }

        let directory = self.root.join(&package.id).join(format!(
            "{}-{}",
            package.version,
            &inspected.sha256[..8]
        ));
        if directory.exists() {
            // The same build again: stop it first, since its files are about
            // to be replaced underneath it.
            lock(&self.registry).unregister(&package.id, &self.vars);
            std::fs::remove_dir_all(&directory)?;
        }
        std::fs::create_dir_all(&directory)?;
        if let Err(error) = unpack(bytes, &directory) {
            let _ = std::fs::remove_dir_all(&directory);
            return Err(error);
        }

        let record = Installed {
            id: package.id.clone(),
            name: package.name.clone(),
            version: package.version.clone(),
            sha256: inspected.sha256,
            permissions: package.permissions.clone(),
            enabled: true,
            source,
            path: directory.clone(),
            installed_at: now_secs(),
        };

        // Start the new build *before* touching the old one. If it does not
        // come up, the old one is still running and nothing has changed.
        let previous = self.installed().into_iter().find(|r| r.id == record.id);
        if let Err(error) = self.start(&record) {
            let _ = std::fs::remove_dir_all(&directory);
            return Err(error);
        }

        self.upsert(record.clone())?;
        if let Some(previous) = previous {
            if previous.path != record.path && !matches!(previous.source, Source::Dev { .. }) {
                let _ = std::fs::remove_dir_all(&previous.path);
            }
        }
        Ok(record)
    }

    /// Run a plugin straight from a working directory, and restart it
    /// whenever a file in it changes. Permissions are taken as declared: the
    /// person linking it is the person writing it.
    pub fn link_dev(&self, path: &Path) -> Result<Installed, PluginsError> {
        let _busy = lock(&self.busy);
        let text = std::fs::read_to_string(path.join(abi::PACKAGE_MANIFEST)).map_err(|error| {
            PluginsError::Package(format!(
                "{} has no readable {}: {error}",
                path.display(),
                abi::PACKAGE_MANIFEST
            ))
        })?;
        let package: abi::PackageManifest = serde_json::from_str(&text)
            .map_err(|error| PluginsError::Package(error.to_string()))?;
        package.validate()?;
        self.refuse_built_in(&package.id)?;

        let path = path.canonicalize()?;
        let record = Installed {
            id: package.id.clone(),
            name: package.name.clone(),
            version: package.version.clone(),
            sha256: String::new(),
            permissions: package.permissions.clone(),
            enabled: true,
            source: Source::Dev { path: path.clone() },
            path,
            installed_at: now_secs(),
        };
        self.start(&record)?;
        self.upsert(record.clone())?;
        Ok(record)
    }

    fn refuse_built_in(&self, id: &str) -> Result<(), PluginsError> {
        if lock(&self.registry).origin(id) == Some(Origin::BuiltIn) {
            return Err(PluginsError::BuiltIn(id.to_string()));
        }
        Ok(())
    }

    /// Start `record` and register it, replacing whatever ran under its id.
    fn start(&self, record: &Installed) -> Result<(), PluginsError> {
        let result = self.start_inner(record);
        let mut failures = lock(&self.failures);
        match &result {
            Ok(()) => {
                failures.remove(&record.id);
            }
            Err(error) => {
                failures.insert(record.id.clone(), error.to_string());
            }
        }
        result
    }

    fn start_inner(&self, record: &Installed) -> Result<(), PluginsError> {
        let text = std::fs::read_to_string(record.path.join(abi::PACKAGE_MANIFEST))?;
        let package: abi::PackageManifest = serde_json::from_str(&text)
            .map_err(|error| PluginsError::Package(error.to_string()))?;
        // The package on disk may not claim more than was agreed to.
        if let Some(extra) = package
            .permissions
            .iter()
            .find(|permission| !record.permissions.contains(permission))
        {
            return Err(PluginsError::Permissions {
                missing: vec![extra.clone()],
                package: Box::new(package),
                sha256: record.sha256.clone(),
            });
        }
        // A recipe runs in the agent's own source engine: no process.
        if package.runtime == abi::runtime::RECIPE {
            let sources = self
                .recipes
                .get()
                .ok_or_else(|| PluginsError::Package("this agent cannot run recipes yet".into()))?;
            let recipe = crate::recipes::RecipePlugin::load(&record.path, &package, Arc::clone(sources))
                .map_err(PluginsError::Package)?;
            lock(&self.registry).register_into(Box::new(recipe), &self.vars);
            let _ = self.events.send(HostEvent::Changed { plugin: record.id.clone() });
            return Ok(());
        }
        let spec = SpawnSpec::for_package(&record.path, &package, &self.runtimes)?;
        let data_dir = self.root.join(".data").join(&record.id);
        std::fs::create_dir_all(&data_dir)?;
        // Stored values only: the plugin fills in its own defaults, since the
        // registry cannot know them until the plugin has said what they are.
        let settings = lock(&self.registry).stored_settings(&record.id);
        let plugin = SidecarPlugin::start(
            spec,
            package,
            settings,
            Services {
                vars: Arc::clone(&self.vars),
                events: self.events.clone(),
            },
            Options {
                data_dir: Some(data_dir),
                ..self.options.clone()
            },
        )?;
        lock(&self.registry).register_into(Box::new(plugin), &self.vars);
        let _ = self.events.send(HostEvent::Changed {
            plugin: record.id.clone(),
        });
        Ok(())
    }

    /// Run recipe packages' sources in `sources`. Once, before [`Self::load_all`].
    pub fn host_recipes(&self, sources: Arc<crate::sources::Sources>) {
        let _ = self.recipes.set(sources);
    }

    /// Start every enabled plugin. One that fails is recorded with its reason
    /// and does not stop the others or the agent.
    pub fn load_all(&self) {
        for record in self.installed() {
            if record.enabled {
                if let Err(error) = self.start(&record) {
                    tracing::warn!("plugin '{}' did not start: {error}", record.id);
                }
            }
        }
    }

    /// Stop and remove a plugin. Its settings are kept, so reinstalling it
    /// later picks up where it left off.
    pub fn uninstall(&self, id: &str) -> Result<(), PluginsError> {
        let _busy = lock(&self.busy);
        let mut records = self.installed();
        let Some(index) = records.iter().position(|record| record.id == id) else {
            return Err(PluginsError::NotInstalled(id.to_string()));
        };
        let record = records.remove(index);
        lock(&self.registry).unregister(id, &self.vars);
        lock(&self.failures).remove(id);
        self.save(&records)?;
        if !matches!(record.source, Source::Dev { .. }) {
            let _ = std::fs::remove_dir_all(self.root.join(id));
        }
        let _ = self.events.send(HostEvent::Changed {
            plugin: id.to_string(),
        });
        Ok(())
    }

    /// Turn a plugin off without removing it, or back on.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), PluginsError> {
        let _busy = lock(&self.busy);
        let mut record = self
            .installed()
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| PluginsError::NotInstalled(id.to_string()))?;
        record.enabled = enabled;
        if enabled {
            self.start(&record)?;
        } else {
            lock(&self.registry).unregister(id, &self.vars);
            lock(&self.failures).remove(id);
        }
        self.upsert(record)?;
        let _ = self.events.send(HostEvent::Changed {
            plugin: id.to_string(),
        });
        Ok(())
    }

    /// Stop and start a plugin — after it gave up crashing, or after its
    /// files changed.
    pub fn reload(&self, id: &str) -> Result<(), PluginsError> {
        let _busy = lock(&self.busy);
        let record = self
            .installed()
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| PluginsError::NotInstalled(id.to_string()))?;
        lock(&self.registry).unregister(id, &self.vars);
        self.start(&record)
    }

    /// The tail of a plugin's log.
    pub fn log(&self, id: &str) -> Vec<String> {
        lock(&self.logs).get(id).cloned().unwrap_or_default()
    }

    /// Remember a log line for [`Self::log`].
    pub fn record_log(&self, id: &str, line: String) {
        let mut logs = lock(&self.logs);
        let lines = logs.entry(id.to_string()).or_default();
        lines.push(line);
        let excess = lines.len().saturating_sub(self.options.log_lines);
        lines.drain(..excess);
    }

    /// Watch every linked plugin's directory and reload it when a file
    /// changes. Polls once a second: a dependency-free watcher that is
    /// plenty for a person saving a file.
    pub fn watch_dev(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("plugin-dev-watch".into())
            .spawn(move || {
                let mut seen: BTreeMap<String, SystemTime> = BTreeMap::new();
                while !manager.stopping.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_secs(1));
                    for record in manager.installed() {
                        let Source::Dev { path } = &record.source else {
                            continue;
                        };
                        if !record.enabled {
                            continue;
                        }
                        let latest = newest_change(path);
                        let previous = seen.insert(record.id.clone(), latest);
                        if previous.is_some_and(|previous| latest > previous) {
                            tracing::info!("plugin '{}' changed; reloading", record.id);
                            if let Err(error) = manager.reload(&record.id) {
                                tracing::warn!("plugin '{}' did not reload: {error}", record.id);
                            }
                        }
                    }
                }
            });
    }

    /// Stop every installed plugin, for shutdown.
    pub fn stop_all(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let mut registry = lock(&self.registry);
        for record in self.installed() {
            registry.unregister(&record.id, &self.vars);
        }
    }
}

/// The newest modification time under `path`, skipping dependency and
/// version-control directories, which change without the plugin changing.
fn newest_change(path: &Path) -> SystemTime {
    let mut newest = SystemTime::UNIX_EPOCH;
    let mut pending = vec![path.to_path_buf()];
    let mut visited = 0usize;
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 20_000 {
                return newest;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "node_modules" || name == ".git" || name == "target" {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if let Ok(modified) = metadata.modified() {
                newest = newest.max(modified);
            }
        }
    }
    newest
}

/// The HTTP client for downloads: rustls with the same crypto provider as
/// the rest of the agent, and a deadline so a stalled server cannot hold an
/// install open forever.
fn http() -> ureq::Agent {
    http_with_timeout(Duration::from_secs(120))
}

/// [`http`], with a different deadline.
pub(crate) fn http_with_timeout(timeout: Duration) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// Download a package, refusing one larger than [`MAX_PACKAGE_BYTES`].
pub fn download(url: &str) -> Result<Vec<u8>, PluginsError> {
    crate::net::check_download_url(url).map_err(PluginsError::Download)?;
    let response = http()
        .get(url)
        .call()
        .map_err(|error| PluginsError::Download(format!("{url}: {error}")))?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_PACKAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PluginsError::Download(format!("{url}: {error}")))?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(PluginsError::Download(format!(
            "{url} is larger than {MAX_PACKAGE_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_from_the_old_marketplace_still_loads() {
        let source: Source = serde_json::from_str(r#"{"kind":"marketplace"}"#).unwrap();
        assert_eq!(source, Source::Marketplace);
        let source: Source = serde_json::from_str(
            r#"{"kind":"repository","url":"https://a.example/repository.min.json","name":"A"}"#,
        )
        .unwrap();
        assert!(matches!(source, Source::Repository { .. }));
    }

    #[test]
    fn a_package_packed_by_the_typescript_sdk_unpacks_here() {
        // Packed by `packages/plugin-sdk`'s own zip writer. Two independent zip
        // implementations agreeing on these bytes is the check that a plugin
        // packed by an author installs on an agent.
        let bytes = include_bytes!("../../../contract/fixtures/plugin/sample.fdpkg");
        let package = read_package_manifest(bytes).expect("the manifest reads");
        assert_eq!(package.id, "sample");
        let dir = tempfile::tempdir().unwrap();
        unpack(bytes, dir.path()).expect("it unpacks");
        let entry = std::fs::read_to_string(dir.path().join("dist/index.js")).unwrap();
        assert_eq!(
            entry,
            "export default {};
"
            .repeat(50)
        );
    }

    #[test]
    fn a_package_without_a_manifest_is_named_as_such() {
        let error = read_package_manifest(b"not a zip").expect_err("refused");
        assert!(matches!(error, PluginsError::Package(_)));
    }
}
