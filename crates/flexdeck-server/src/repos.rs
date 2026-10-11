//! Plugin repositories: where the marketplace's plugins come from.
//!
//! # Three kinds
//!
//! * **Official** — flexdeck's own repository. Always there, never removable.
//! * **Vetted** — third-party repositories flexdeck has reviewed, read from a
//!   list published next to the official repository, so the list can change
//!   without a release. Each can be turned off.
//! * **Custom** — any repository a person adds by address. Shown as
//!   unreviewed, because nobody but the person adding it vouched for it.
//!
//! A repository is a static `repository.min.json` (see [`abi::repo`]). The
//! marketplace is every enabled repository's plugins merged into one list;
//! installing one fetches its package and refuses it unless the bytes hash to
//! the SHA-256 the index pinned.
//!
//! # Offline
//!
//! The last good copy of every index is kept on disk, so the marketplace still
//! lists what it knew when the network is down — with the error beside it,
//! rather than an empty page.
//!
//! # Icons
//!
//! The window's content policy allows images only from itself and `data:`
//! URIs, and the portal reaches the agent through a relay. So the agent
//! fetches icons itself — PNG, JPEG or WebP, recognised by their bytes and
//! never by a header, and small — and hands them over as `data:` URIs. A
//! webview never contacts a repository's host, and SVG (which can carry
//! script) is never accepted.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use flexdeck_plugin::abi;
use flexdeck_plugin::abi::repo::{
    RepoIndex, RepoPlugin, VettedList, VettedRepo, MAX_INDEX_BYTES, REPO_FILE,
};
use flexdeck_store::ConfigStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::plugins::{Installed, PluginsError, Source};

/// Metadata key for the person's repository choices.
pub const REPOS_KEY: &str = "plugins.repositories";

/// flexdeck's own repository.
pub const OFFICIAL_REPO_URL: &str =
    "https://raw.githubusercontent.com/flexdeck/plugins/main/repository.min.json";
/// The list of vetted third-party repositories.
pub const VETTED_LIST_URL: &str =
    "https://raw.githubusercontent.com/flexdeck/plugins/main/repositories.min.json";

/// Overrides [`OFFICIAL_REPO_URL`], for development and tests.
pub const OFFICIAL_ENV: &str = "FLEXDECK_OFFICIAL_REPO";
/// Overrides [`VETTED_LIST_URL`]; set it empty to read no vetted list.
pub const VETTED_ENV: &str = "FLEXDECK_VETTED_REPOS";

/// How long a fetched index is trusted before it is fetched again.
pub const TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// The deadline for fetching one index.
const INDEX_TIMEOUT: Duration = Duration::from_secs(15);
/// The deadline for fetching one icon.
const ICON_TIMEOUT: Duration = Duration::from_secs(8);
/// The largest icon kept.
pub const MAX_ICON_BYTES: usize = 128 * 1024;
/// The most fetches at once.
const PARALLEL: usize = 8;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn short_hash(text: &str) -> String {
    hex::encode(&Sha256::digest(text.as_bytes())[..8])
}

// -- addresses ------------------------------------------------------------------

fn is_loopback(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// Only `https`, or `http` to this machine: a package fetched in the clear
/// could be swapped on the way, index and all.
fn check_scheme(url: &url::Url) -> Result<(), PluginsError> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if url.host().as_ref().is_some_and(is_loopback) => Ok(()),
        "http" => Err(PluginsError::Repository(format!(
            "{url} is not encrypted; use an https:// address"
        ))),
        _ => Err(PluginsError::Repository(format!(
            "'{url}' is not a web address"
        ))),
    }
}

/// Turn what a person typed into the address of an index.
///
/// `xyz.com` becomes `https://xyz.com/repository.min.json`, a folder gets
/// `/repository.min.json` added, and an address naming a `.json` file is kept.
pub fn normalize_repo_url(input: &str) -> Result<String, PluginsError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(PluginsError::Repository("type a repository address".into()));
    }
    let with_scheme = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let mut url = url::Url::parse(&with_scheme).map_err(|error| {
        PluginsError::Repository(format!("'{input}' is not an address: {error}"))
    })?;
    check_scheme(&url)?;
    if url.host_str().is_none_or(str::is_empty) {
        return Err(PluginsError::Repository(format!("'{input}' has no host")));
    }
    url.set_fragment(None);
    if !url.path().to_ascii_lowercase().ends_with(".json") {
        let path = format!("{}/{REPO_FILE}", url.path().trim_end_matches('/'));
        url.set_path(&path);
    }
    Ok(url.to_string())
}

/// Resolve an address written in an index against the index's own address.
pub fn resolve(base: &str, reference: &str) -> Result<String, PluginsError> {
    let base = url::Url::parse(base)
        .map_err(|error| PluginsError::Repository(format!("{base}: {error}")))?;
    let url = base
        .join(reference.trim())
        .map_err(|error| PluginsError::Repository(format!("'{reference}': {error}")))?;
    check_scheme(&url)?;
    Ok(url.to_string())
}

// -- fetching -------------------------------------------------------------------

enum Fetched {
    NotModified,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

/// GET `url`, refusing more than `cap` bytes, with `If-None-Match` when an
/// earlier copy's tag is known.
fn fetch_capped(
    url: &str,
    cap: usize,
    timeout: Duration,
    etag: Option<&str>,
) -> Result<Fetched, String> {
    let mut request = crate::plugins::http_with_timeout(timeout).get(url);
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    let response = request.call().map_err(|error| error.to_string())?;
    if response.status() == 304 {
        return Ok(Fetched::NotModified);
    }
    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > cap {
        return Err(format!("it is larger than {cap} bytes"));
    }
    Ok(Fetched::Body { bytes, etag })
}

/// The image type of `bytes`, by their first bytes.
fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Run `work` over `items`, at most [`PARALLEL`] at a time.
fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let mut results = Vec::with_capacity(items.len());
    for chunk in items.chunks(PARALLEL) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|item| scope.spawn(|| work(item)))
                .collect();
            for handle in handles {
                results.push(handle.join().expect("a fetch thread does not panic"));
            }
        });
    }
    results
}

// -- state ----------------------------------------------------------------------

/// Which repository list an address came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Official,
    Vetted,
    Custom,
}

/// Where the official repository and the vetted list are.
#[derive(Debug, Clone)]
pub struct Sources {
    pub official: String,
    pub vetted_list: Option<String>,
}

impl Sources {
    /// The built-in addresses, unless the environment overrides them.
    pub fn from_env() -> Self {
        let official = std::env::var(OFFICIAL_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| OFFICIAL_REPO_URL.to_string());
        let vetted_list = match std::env::var(VETTED_ENV) {
            Ok(value) if value.trim().is_empty() => None,
            Ok(value) => Some(value),
            Err(_) => Some(VETTED_LIST_URL.to_string()),
        };
        Self {
            official,
            vetted_list,
        }
    }
}

/// A repository the person added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomRepo {
    url: String,
    added_at: u64,
}

/// The person's choices, persisted under [`REPOS_KEY`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Choices {
    #[serde(default)]
    custom: Vec<CustomRepo>,
    /// Vetted repositories turned off.
    #[serde(default)]
    vetted_off: BTreeSet<String>,
}

/// What is known about one repository.
#[derive(Debug, Clone, Default)]
struct Cached {
    index: Option<RepoIndex>,
    dropped: Vec<String>,
    etag: Option<String>,
    /// When the index in hand was fetched (seconds since the epoch).
    fetched_at: Option<u64>,
    /// When it was last *tried*, this run.
    checked: Option<Instant>,
    error: Option<String>,
}

/// What is kept on disk per repository.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiskCopy {
    url: String,
    etag: Option<String>,
    fetched_at: u64,
    index: Value,
}

#[derive(Default)]
struct State {
    vetted: Vec<VettedRepo>,
    vetted_checked: Option<Instant>,
    vetted_error: Option<String>,
    repos: BTreeMap<String, Cached>,
    /// Icon address -> `data:` URI, or `None` when it could not be used.
    icons: BTreeMap<String, Option<String>>,
}

/// One repository as the window lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRow {
    pub url: String,
    pub name: String,
    pub description: String,
    pub website: Option<String>,
    pub kind: Kind,
    pub enabled: bool,
    pub plugin_count: usize,
    pub fetched_at: Option<u64>,
    pub error: Option<String>,
    /// Entries the index listed but that could not be read.
    pub dropped: Vec<String>,
}

/// What adding a repository would add, before it is added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub url: String,
    pub name: String,
    pub description: String,
    pub website: Option<String>,
    pub plugin_count: usize,
    pub plugins: Vec<String>,
    pub kind: Kind,
}

/// One entry found in a repository, ready to install.
#[derive(Debug, Clone)]
pub struct Found {
    pub repo_url: String,
    pub repo_name: String,
    pub plugin: RepoPlugin,
    pub package_url: String,
}

/// The repositories, their cached indexes, and the person's choices.
pub struct Repositories {
    sources: Sources,
    cache_dir: PathBuf,
    store: Arc<Mutex<ConfigStore>>,
    state: Mutex<State>,
    /// One refresh at a time; a second caller waits and then finds it fresh.
    refreshing: Mutex<()>,
}

impl Repositories {
    /// Repositories whose indexes are cached under `cache_dir`.
    pub fn new(sources: Sources, cache_dir: PathBuf, store: Arc<Mutex<ConfigStore>>) -> Self {
        let repositories = Self {
            sources,
            cache_dir,
            store,
            state: Mutex::new(State::default()),
            refreshing: Mutex::new(()),
        };
        repositories.load_disk_copies();
        repositories
    }

    fn choices(&self) -> Choices {
        lock(&self.store)
            .meta(REPOS_KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    fn save_choices(&self, choices: &Choices) -> Result<(), PluginsError> {
        let json = serde_json::to_string(choices).expect("choices always serialise");
        lock(&self.store).set_meta(REPOS_KEY, &json)?;
        Ok(())
    }

    fn disk_path(&self, url: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.json", short_hash(url)))
    }

    fn icon_path(&self, url: &str) -> PathBuf {
        self.cache_dir.join("icons").join(short_hash(url))
    }

    /// Read every index kept from an earlier run, so the marketplace lists
    /// something before the first fetch finishes, or without a network.
    fn load_disk_copies(&self) {
        let Ok(entries) = std::fs::read_dir(&self.cache_dir) else {
            return;
        };
        let mut state = lock(&self.state);
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let Some(copy) = std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<DiskCopy>(&bytes).ok())
            else {
                continue;
            };
            if copy.url == VETTED_KEY {
                if let Ok(list) = serde_json::from_value::<VettedList>(copy.index) {
                    state.vetted = list.repositories;
                }
                continue;
            }
            let Ok(parsed) = RepoIndex::parse(copy.index.to_string().as_bytes()) else {
                continue;
            };
            state.repos.insert(
                copy.url,
                Cached {
                    index: Some(parsed.index),
                    dropped: parsed.dropped,
                    etag: copy.etag,
                    fetched_at: Some(copy.fetched_at),
                    ..Cached::default()
                },
            );
        }
    }

    fn write_disk_copy(&self, url: &str, etag: Option<&str>, index: Value) {
        let copy = DiskCopy {
            url: url.to_string(),
            etag: etag.map(str::to_string),
            fetched_at: now_secs(),
            index,
        };
        let _ = std::fs::create_dir_all(&self.cache_dir);
        let path = if url == VETTED_KEY {
            self.cache_dir.join("vetted.json")
        } else {
            self.disk_path(url)
        };
        if let Ok(bytes) = serde_json::to_vec(&copy) {
            let partial = path.with_extension("partial");
            if std::fs::write(&partial, bytes).is_ok() {
                let _ = std::fs::rename(&partial, &path);
            }
        }
    }

    /// Every repository the marketplace reads, in display order: official,
    /// then vetted, then the person's own.
    fn all(&self) -> Vec<(String, Kind, bool)> {
        let choices = self.choices();
        let state = lock(&self.state);
        let mut seen = BTreeSet::new();
        let mut all = Vec::new();
        let mut push = |url: String, kind: Kind, enabled: bool| {
            if seen.insert(url.clone()) {
                all.push((url, kind, enabled));
            }
        };
        push(self.sources.official.clone(), Kind::Official, true);
        for vetted in &state.vetted {
            if let Ok(url) = normalize_repo_url(&vetted.url) {
                let enabled = !choices.vetted_off.contains(&url);
                push(url, Kind::Vetted, enabled);
            }
        }
        for custom in choices.custom {
            push(custom.url, Kind::Custom, true);
        }
        all
    }

    fn kind_of(&self, url: &str) -> Option<Kind> {
        self.all()
            .into_iter()
            .find(|(known, _, _)| known == url)
            .map(|(_, kind, _)| kind)
    }

    /// Fetch the vetted list and every enabled repository that is due —
    /// every one, when `force`.
    pub fn refresh(&self, force: bool) {
        let _one_at_a_time = lock(&self.refreshing);
        self.refresh_vetted(force);
        let all = self.all();
        let due: Vec<String> = {
            let state = lock(&self.state);
            all.into_iter()
                .filter(|(_, _, enabled)| *enabled)
                .map(|(url, _, _)| url)
                .filter(|url| {
                    force
                        || state
                            .repos
                            .get(url)
                            .and_then(|cached| cached.checked)
                            .is_none_or(|checked| checked.elapsed() >= TTL)
                })
                .collect()
        };
        let etags: Vec<(String, Option<String>)> = {
            let state = lock(&self.state);
            due.into_iter()
                .map(|url| {
                    let etag = state
                        .repos
                        .get(&url)
                        .and_then(|cached| cached.index.as_ref().and(cached.etag.clone()));
                    (url, etag)
                })
                .collect()
        };
        let results = in_parallel(&etags, |(url, etag)| {
            fetch_capped(url, MAX_INDEX_BYTES, INDEX_TIMEOUT, etag.as_deref())
        });
        for ((url, _), result) in etags.into_iter().zip(results) {
            self.record(&url, result);
        }
    }

    fn refresh_vetted(&self, force: bool) {
        let Some(list_url) = self.sources.vetted_list.clone() else {
            return;
        };
        let due = force
            || lock(&self.state)
                .vetted_checked
                .is_none_or(|checked| checked.elapsed() >= TTL);
        if !due {
            return;
        }
        let outcome =
            fetch_capped(&list_url, MAX_INDEX_BYTES, INDEX_TIMEOUT, None).and_then(|fetched| {
                match fetched {
                    Fetched::Body { bytes, .. } => VettedList::parse(&bytes)
                        .map_err(|error| error.to_string())
                        .map(|list| (list, bytes)),
                    Fetched::NotModified => Err("the server sent nothing new".into()),
                }
            });
        let mut state = lock(&self.state);
        state.vetted_checked = Some(Instant::now());
        match outcome {
            Ok((list, bytes)) => {
                state.vetted = list.repositories;
                state.vetted_error = None;
                drop(state);
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    self.write_disk_copy(VETTED_KEY, None, value);
                }
            }
            Err(error) => state.vetted_error = Some(error),
        }
    }

    fn record(&self, url: &str, result: Result<Fetched, String>) {
        let parsed = result.and_then(|fetched| match fetched {
            Fetched::NotModified => Ok(None),
            Fetched::Body { bytes, etag } => RepoIndex::parse(&bytes)
                .map_err(|error| error.to_string())
                .map(|parsed| Some((parsed, etag, bytes))),
        });
        let mut state = lock(&self.state);
        let cached = state.repos.entry(url.to_string()).or_default();
        cached.checked = Some(Instant::now());
        match parsed {
            Ok(None) => {
                cached.error = None;
                cached.fetched_at = Some(now_secs());
            }
            Ok(Some((parsed, etag, bytes))) => {
                cached.index = Some(parsed.index);
                cached.dropped = parsed.dropped;
                cached.etag.clone_from(&etag);
                cached.fetched_at = Some(now_secs());
                cached.error = None;
                drop(state);
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    self.write_disk_copy(url, etag.as_deref(), value);
                }
            }
            // The last good copy stays; the error is shown beside it.
            Err(error) => cached.error = Some(error),
        }
    }

    /// Every repository, for the repositories list.
    pub fn list(&self) -> Vec<RepoRow> {
        let all = self.all();
        let state = lock(&self.state);
        all.into_iter()
            .map(|(url, kind, enabled)| {
                let cached = state.repos.get(&url).cloned().unwrap_or_default();
                let vetted = state
                    .vetted
                    .iter()
                    .find(|vetted| normalize_repo_url(&vetted.url).ok().as_deref() == Some(&url));
                let meta = cached.index.as_ref().map(|index| &index.repo);
                let name = meta
                    .map(|meta| meta.name.clone())
                    .filter(|name| !name.is_empty())
                    .or_else(|| vetted.map(|vetted| vetted.name.clone()))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| match kind {
                        Kind::Official => "flexdeck".to_string(),
                        _ => host_of(&url),
                    });
                RepoRow {
                    name,
                    description: meta
                        .map(|meta| meta.description.clone())
                        .filter(|text| !text.is_empty())
                        .or_else(|| vetted.map(|vetted| vetted.description.clone()))
                        .unwrap_or_default(),
                    website: meta.and_then(|meta| meta.website.clone()),
                    plugin_count: cached.index.as_ref().map_or(0, |index| index.plugins.len()),
                    fetched_at: cached.fetched_at,
                    error: cached.error,
                    dropped: cached.dropped,
                    url,
                    kind,
                    enabled,
                }
            })
            .collect()
    }

    /// The vetted list's own fetch error, if its last fetch failed.
    pub fn vetted_error(&self) -> Option<String> {
        lock(&self.state).vetted_error.clone()
    }

    /// Fetch and read a repository without adding it.
    pub fn preview(&self, input: &str) -> Result<Preview, PluginsError> {
        let url = normalize_repo_url(input)?;
        let bytes = match fetch_capped(&url, MAX_INDEX_BYTES, INDEX_TIMEOUT, None) {
            Ok(Fetched::Body { bytes, .. }) => bytes,
            Ok(Fetched::NotModified) => {
                return Err(PluginsError::Repository(format!("{url} sent nothing")))
            }
            Err(error) => {
                return Err(PluginsError::Download(format!("{url}: {error}")));
            }
        };
        let parsed = RepoIndex::parse(&bytes)
            .map_err(|error| PluginsError::Repository(error.to_string()))?;
        let kind = self.kind_of(&url).unwrap_or(Kind::Custom);
        let mut plugins: Vec<String> = parsed
            .index
            .plugins
            .iter()
            .map(|plugin| plugin.name.clone())
            .collect();
        plugins.sort();
        Ok(Preview {
            name: if parsed.index.repo.name.is_empty() {
                host_of(&url)
            } else {
                parsed.index.repo.name
            },
            description: parsed.index.repo.description,
            website: parsed.index.repo.website,
            plugin_count: plugins.len(),
            plugins,
            url,
            kind,
        })
    }

    /// Add a repository by address. Fetches it first: an address that does
    /// not answer with a readable index is refused, not saved.
    pub fn add(&self, input: &str) -> Result<RepoRow, PluginsError> {
        let url = normalize_repo_url(input)?;
        match self.kind_of(&url) {
            Some(Kind::Official) => {
                return Err(PluginsError::Repository(
                    "that is the official repository; it is always there".into(),
                ))
            }
            Some(Kind::Vetted) => {
                // Already listed as vetted: adding it means "turn it on".
                self.set_vetted_enabled(&url, true)?;
            }
            Some(Kind::Custom) => {}
            None => {
                let result = fetch_capped(&url, MAX_INDEX_BYTES, INDEX_TIMEOUT, None);
                let bytes = match result {
                    Ok(Fetched::Body { ref bytes, .. }) => bytes.clone(),
                    Ok(Fetched::NotModified) => Vec::new(),
                    Err(error) => return Err(PluginsError::Download(format!("{url}: {error}"))),
                };
                RepoIndex::parse(&bytes)
                    .map_err(|error| PluginsError::Repository(error.to_string()))?;
                let mut choices = self.choices();
                choices.custom.push(CustomRepo {
                    url: url.clone(),
                    added_at: now_secs(),
                });
                self.save_choices(&choices)?;
                self.record(&url, result);
            }
        }
        self.list()
            .into_iter()
            .find(|row| row.url == url)
            .ok_or_else(|| PluginsError::Repository(format!("{url} was not added")))
    }

    /// Remove a repository the person added. Plugins installed from it stay
    /// installed; they just stop being offered updates.
    pub fn remove(&self, url: &str) -> Result<(), PluginsError> {
        match self.kind_of(url) {
            Some(Kind::Custom) => {}
            Some(Kind::Official) => {
                return Err(PluginsError::Repository(
                    "the official repository cannot be removed".into(),
                ))
            }
            Some(Kind::Vetted) => {
                return Err(PluginsError::Repository(
                    "a vetted repository is turned off, not removed".into(),
                ))
            }
            None => return Err(PluginsError::Repository(format!("{url} is not added"))),
        }
        let mut choices = self.choices();
        choices.custom.retain(|custom| custom.url != url);
        self.save_choices(&choices)?;
        lock(&self.state).repos.remove(url);
        let _ = std::fs::remove_file(self.disk_path(url));
        Ok(())
    }

    /// Turn a vetted repository on or off.
    pub fn set_vetted_enabled(&self, url: &str, enabled: bool) -> Result<(), PluginsError> {
        if self.kind_of(url) != Some(Kind::Vetted) {
            return Err(PluginsError::Repository(format!(
                "{url} is not a vetted repository"
            )));
        }
        let mut choices = self.choices();
        if enabled {
            choices.vetted_off.remove(url);
        } else {
            choices.vetted_off.insert(url.to_string());
        }
        self.save_choices(&choices)
    }

    /// Every plugin in every enabled repository whose id, name, description or
    /// author contains `query` (any case), with its icon and how it relates to
    /// what is installed.
    pub fn catalog(&self, query: &str, installed: &[Installed]) -> Vec<Value> {
        let query = query.trim().to_lowercase();
        let rows = self.list();
        let mut found: Vec<(RepoRow, RepoPlugin)> = Vec::new();
        {
            let state = lock(&self.state);
            for row in rows.iter().filter(|row| row.enabled) {
                let Some(index) = state.repos.get(&row.url).and_then(|c| c.index.as_ref()) else {
                    continue;
                };
                for plugin in &index.plugins {
                    let matches = query.is_empty()
                        || [
                            &plugin.id,
                            &plugin.name,
                            &plugin.description,
                            &plugin.author,
                        ]
                        .iter()
                        .any(|field| field.to_lowercase().contains(&query));
                    if matches {
                        found.push((row.clone(), plugin.clone()));
                    }
                }
            }
        }

        let icon_urls: Vec<Option<String>> = found
            .iter()
            .map(|(row, plugin)| {
                plugin
                    .icon_for(128)
                    .and_then(|icon| resolve(&row.url, icon).ok())
            })
            .collect();
        self.fetch_icons(&icon_urls);
        let icons = lock(&self.state).icons.clone();

        found
            .into_iter()
            .zip(icon_urls)
            .map(|((row, plugin), icon_url)| {
                let current = installed.iter().find(|record| record.id == plugin.id);
                let from_here = current.is_some_and(|record| {
                    matches!(&record.source, Source::Repository { url, .. } if *url == row.url)
                });
                let compatible = runs_here(&plugin);
                let update = from_here
                    && compatible
                    && current.is_some_and(|record| {
                        abi::repo::version_order(&plugin.version, &record.version).is_gt()
                    });
                let mut entry = serde_json::to_value(&plugin).expect("an entry serialises");
                entry["repo"] = json!({ "url": row.url, "name": row.name, "kind": row.kind });
                entry["iconData"] =
                    json!(icon_url.and_then(|url| icons.get(&url).cloned().flatten()));
                entry["installedVersion"] = json!(current.map(|record| &record.version));
                entry["installedFromHere"] = json!(from_here);
                entry["update"] = json!(update);
                // Shown, not hidden: "needs a newer flexdeck" is more useful
                // than a plugin that is simply missing from the list.
                entry["compatible"] = json!(compatible);
                entry
            })
            .collect()
    }

    /// Fetch icons not yet known, from memory, disk, or the network.
    fn fetch_icons(&self, urls: &[Option<String>]) {
        let wanted: Vec<String> = {
            let state = lock(&self.state);
            urls.iter()
                .flatten()
                .filter(|url| !state.icons.contains_key(*url))
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };
        if wanted.is_empty() {
            return;
        }
        let results = in_parallel(&wanted, |url| {
            let path = self.icon_path(url);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => match fetch_capped(url, MAX_ICON_BYTES, ICON_TIMEOUT, None) {
                    Ok(Fetched::Body { bytes, .. }) if image_type(&bytes).is_some() => {
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        let _ = std::fs::write(&path, &bytes);
                        bytes
                    }
                    _ => return None,
                },
            };
            use base64::Engine as _;
            image_type(&bytes).map(|mime| {
                format!(
                    "data:{mime};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                )
            })
        });
        let mut state = lock(&self.state);
        for (url, data) in wanted.into_iter().zip(results) {
            state.icons.insert(url, data);
        }
    }

    /// Find `id` in the repository at `repo_url`, refreshing that repository
    /// first when it has never been read.
    pub fn find(&self, repo_url: &str, id: &str) -> Result<Found, PluginsError> {
        abi::validate_id(id)?;
        if self.kind_of(repo_url).is_none() {
            return Err(PluginsError::Repository(format!(
                "{repo_url} is not one of this desktop's repositories"
            )));
        }
        let known = lock(&self.state)
            .repos
            .get(repo_url)
            .is_some_and(|cached| cached.index.is_some());
        if !known {
            let result = fetch_capped(repo_url, MAX_INDEX_BYTES, INDEX_TIMEOUT, None);
            self.record(repo_url, result);
        }
        let name = self
            .list()
            .into_iter()
            .find(|row| row.url == repo_url)
            .map_or_else(|| host_of(repo_url), |row| row.name);
        let state = lock(&self.state);
        let cached = state.repos.get(repo_url);
        let Some(index) = cached.and_then(|cached| cached.index.as_ref()) else {
            let why = cached
                .and_then(|cached| cached.error.clone())
                .unwrap_or_else(|| "it has not been read".into());
            return Err(PluginsError::Download(format!("{repo_url}: {why}")));
        };
        let plugin = index
            .plugin(id)
            .cloned()
            .ok_or_else(|| PluginsError::Repository(format!("{name} does not list '{id}'")))?;
        if !runs_here(&plugin) {
            return Err(PluginsError::Repository(format!(
                "{} {} needs plugin ABI {}, and this flexdeck runs ABI {}; update flexdeck first",
                plugin.name,
                plugin.version,
                plugin.abi,
                abi::SUPPORTED_ABIS
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(" or ")
            )));
        }
        let package_url = resolve(repo_url, &plugin.package.url)?;
        Ok(Found {
            repo_url: repo_url.to_string(),
            repo_name: name,
            plugin,
            package_url,
        })
    }

    /// Download what [`Self::find`] found, returning the bytes and the
    /// checksum they must have. The package's own `plugin.json` must agree
    /// with the index on id and version: an index that describes one plugin
    /// and serves another is refused.
    pub fn download(&self, found: &Found) -> Result<(Vec<u8>, String), PluginsError> {
        let bytes = crate::plugins::download(&found.package_url)?;
        let manifest = crate::plugins::read_package_manifest(&bytes)?;
        if manifest.id != found.plugin.id || manifest.version != found.plugin.version {
            return Err(PluginsError::Repository(format!(
                "{} lists {} {}, but its package is {} {}",
                found.repo_name,
                found.plugin.id,
                found.plugin.version,
                manifest.id,
                manifest.version
            )));
        }
        Ok((bytes, found.plugin.package.sha256.clone()))
    }

    /// Find, download and check `id` from `repo_url`: the bytes, the checksum
    /// they must have, and the source to remember.
    pub fn fetch_package(
        &self,
        repo_url: &str,
        id: &str,
    ) -> Result<(Vec<u8>, String, Source), PluginsError> {
        let found = self.find(repo_url, id)?;
        let (bytes, sha256) = self.download(&found)?;
        Ok((
            bytes,
            sha256,
            Source::Repository {
                url: found.repo_url,
                name: found.repo_name,
            },
        ))
    }

    /// Installed plugins whose own repository lists a newer version.
    ///
    /// Only the repository a plugin came from is asked: another repository
    /// publishing the same id is a different plugin, as far as updates go.
    pub fn updates(&self, installed: &[Installed]) -> Vec<Value> {
        let state = lock(&self.state);
        installed
            .iter()
            .filter_map(|record| {
                let Source::Repository { url, name } = &record.source else {
                    return None;
                };
                let plugin = state.repos.get(url)?.index.as_ref()?.plugin(&record.id)?;
                // An update this agent could not run is no update at all.
                (runs_here(plugin)
                    && abi::repo::version_order(&plugin.version, &record.version).is_gt())
                    .then(|| {
                        json!({
                            "id": record.id,
                            "name": plugin.name,
                            "installed": record.version,
                            "available": plugin.version,
                            "repo": { "url": url, "name": name },
                            "permissions": plugin.permissions,
                        })
                    })
            })
            .collect()
    }
}

/// Whether this agent can run a listed plugin. An index entry that does not say
/// (`abi` absent, read as 0) is given the benefit of the doubt: the package's
/// own `plugin.json` is checked again at install, and refuses with a reason.
fn runs_here(plugin: &RepoPlugin) -> bool {
    plugin.abi == 0 || abi::supports_abi(plugin.abi)
}

/// The key the vetted list's disk copy is stored under.
const VETTED_KEY: &str = "vetted:list";

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_address_becomes_an_index_address() {
        assert_eq!(
            normalize_repo_url("xyz.com").unwrap(),
            "https://xyz.com/repository.min.json"
        );
        assert_eq!(
            normalize_repo_url(" xyz.com/plugins/ ").unwrap(),
            "https://xyz.com/plugins/repository.min.json"
        );
        let raw = "https://raw.githubusercontent.com/someone/deck/main/repository.min.json";
        assert_eq!(normalize_repo_url(raw).unwrap(), raw);
        assert_eq!(
            normalize_repo_url("https://xyz.com/index.json#top").unwrap(),
            "https://xyz.com/index.json"
        );
    }

    #[test]
    fn an_unencrypted_address_is_refused_except_to_this_machine() {
        assert!(normalize_repo_url("http://evil.example").is_err());
        assert!(normalize_repo_url("ftp://xyz.com").is_err());
        assert!(normalize_repo_url("").is_err());
        assert_eq!(
            normalize_repo_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080/repository.min.json"
        );
        assert!(normalize_repo_url("http://localhost:3000/r.json").is_ok());
    }

    #[test]
    fn addresses_in_an_index_resolve_against_it() {
        let base = "https://raw.githubusercontent.com/a/b/main/repository.min.json";
        assert_eq!(
            resolve(base, "packages/obs-1.0.0.fdpkg").unwrap(),
            "https://raw.githubusercontent.com/a/b/main/packages/obs-1.0.0.fdpkg"
        );
        assert_eq!(
            resolve(base, "https://cdn.example/obs.fdpkg").unwrap(),
            "https://cdn.example/obs.fdpkg"
        );
        assert!(
            resolve(base, "http://cdn.example/obs.fdpkg").is_err(),
            "an index cannot downgrade a package to plain http"
        );
    }

    #[test]
    fn icons_are_recognised_by_their_bytes() {
        assert_eq!(image_type(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(image_type(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_type(b"<svg onload=alert(1)>"), None);
    }

    fn repositories(dir: &std::path::Path) -> Repositories {
        let store = ConfigStore::open_in_memory().unwrap();
        Repositories::new(
            Sources {
                official: "https://official.example/repository.min.json".into(),
                vetted_list: None,
            },
            dir.to_path_buf(),
            Arc::new(Mutex::new(store)),
        )
    }

    #[test]
    fn the_official_repository_is_always_listed_and_cannot_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let repos = repositories(dir.path());
        let rows = repos.list();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, Kind::Official);
        assert!(repos.remove(&rows[0].url).is_err());
        assert!(repos.add("official.example").is_err());
    }

    fn index_at(version: &str, abi: u32) -> abi::repo::RepoIndex {
        RepoIndex::parse(
            json!({ "schema": 1, "repo": { "name": "r" }, "plugins": [{
                "id": "obs", "name": "OBS", "version": version, "abi": abi,
                "package": { "url": "obs.fdpkg", "sha256": "0".repeat(64) }
            }]})
            .to_string()
            .as_bytes(),
        )
        .unwrap()
        .index
    }

    #[test]
    fn updates_only_come_from_the_repository_a_plugin_came_from() {
        let dir = tempfile::tempdir().unwrap();
        let repos = repositories(dir.path());
        let index = |version: &str| index_at(version, 2);
        {
            let mut state = lock(&repos.state);
            state.repos.insert(
                "https://a.example/repository.min.json".into(),
                Cached {
                    index: Some(index("2.0.0")),
                    ..Cached::default()
                },
            );
            state.repos.insert(
                "https://b.example/repository.min.json".into(),
                Cached {
                    index: Some(index("9.0.0")),
                    ..Cached::default()
                },
            );
        }
        let record = |url: &str| Installed {
            id: "obs".into(),
            name: "OBS".into(),
            version: "1.0.0".into(),
            sha256: String::new(),
            permissions: vec![],
            enabled: true,
            source: Source::Repository {
                url: url.into(),
                name: "a".into(),
            },
            path: PathBuf::new(),
            installed_at: 0,
        };
        let updates = repos.updates(&[record("https://a.example/repository.min.json")]);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["available"], "2.0.0", "not b's 9.0.0");

        // A newer version this agent cannot run is not offered.
        lock(&repos.state)
            .repos
            .get_mut("https://a.example/repository.min.json")
            .unwrap()
            .index = Some(index_at("3.0.0", 99));
        assert!(
            repos
                .updates(&[record("https://a.example/repository.min.json")])
                .is_empty(),
            "ABI 99 is not an update for this agent"
        );
        let entry = |abi| index_at("1.0.0", abi).plugins.remove(0);
        assert!(runs_here(&entry(2)) && runs_here(&entry(3)));
        assert!(runs_here(&entry(0)), "an index that does not say is checked at install");
        assert!(!runs_here(&entry(1)) && !runs_here(&entry(99)));
        let mut file = record("x");
        file.source = Source::File;
        assert!(
            repos.updates(&[file]).is_empty(),
            "a file install has no repository"
        );
    }
}
