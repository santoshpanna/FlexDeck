//! A plugin repository: a static `repository.min.json` anyone can host.
//!
//! # Why a file and not a service
//!
//! A repository is a list of plugins, each pointing at a `.fdpkg` and pinning
//! its SHA-256. Hosting one needs nothing but a place to put files — GitHub
//! Pages, a raw GitHub URL, any web server — so a plugin author never needs an
//! account with us to publish. flexdeck ships one *official* repository and a
//! list of *vetted* ones; a person can add any other by URL.
//!
//! # What the checksum does and does not prove
//!
//! The SHA-256 comes from the index, and the index comes from the repository.
//! So it proves the package is the one this repository lists — not that the
//! repository is honest. That is why a repository someone adds by hand is shown
//! as unreviewed.
//!
//! # Tolerant reading
//!
//! Unlike the plugin protocol, unknown fields here are *ignored*: an index is
//! written by people with their own tools and read by agents of every age, and
//! a newer field must not hide a whole repository from an older agent. One bad
//! entry is dropped and reported; the rest of the repository still lists.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{validate_id, validate_version};

/// The index format this agent reads.
pub const REPO_SCHEMA: u32 = 1;

/// The file a repository URL names when it does not name a file.
pub const REPO_FILE: &str = "repository.min.json";

/// The largest index accepted.
pub const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepoError {
    #[error("this is not a flexdeck plugin repository: {0}")]
    Unreadable(String),

    #[error("this repository uses format {theirs}, but this agent reads format {ours} (update flexdeck)")]
    Schema { theirs: u32, ours: u32 },

    #[error("'{0}' is listed more than once in this repository")]
    Duplicate(String),
}

/// What a repository says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Where a plugin's package is, and what its bytes must hash to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoPackage {
    /// Absolute, or relative to the index's own URL.
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

/// One plugin, at its latest version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default)]
    pub abi: u32,
    #[serde(default)]
    pub permissions: Vec<String>,
    /// When the plugin was first released (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    /// When this version was published (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_update_date: Option<String>,
    /// The main icon. Absolute, or relative to the index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Icons by pixel size, e.g. `{"64": "icons/obs-64.png"}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icons: BTreeMap<String, String>,
    pub package: RepoPackage,
}

impl RepoPlugin {
    /// Why this entry cannot be installed, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if let Err(error) = validate_id(&self.id) {
            return Some(error.to_string());
        }
        if let Err(error) = validate_version(&self.version) {
            return Some(format!("{}: {error}", self.id));
        }
        let sha = &self.package.sha256;
        if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(format!(
                "{}: the package checksum is not a SHA-256",
                self.id
            ));
        }
        if self.package.url.trim().is_empty() {
            return Some(format!("{}: there is no package address", self.id));
        }
        None
    }

    /// The icon closest to `size` pixels without going under, or the largest,
    /// falling back to [`Self::icon`].
    pub fn icon_for(&self, size: u32) -> Option<&str> {
        let mut sized: Vec<(u32, &String)> = self
            .icons
            .iter()
            .filter_map(|(key, url)| key.parse().ok().map(|px| (px, url)))
            .collect();
        sized.sort_by_key(|(px, _)| *px);
        sized
            .iter()
            .find(|(px, _)| *px >= size)
            .or(sized.last())
            .map(|(_, url)| url.as_str())
            .or(self.icon.as_deref())
    }
}

/// A whole `repository.min.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoIndex {
    pub schema: u32,
    pub repo: RepoMeta,
    #[serde(default)]
    pub plugins: Vec<RepoPlugin>,
}

/// An index, and what had to be left out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub index: RepoIndex,
    /// One line per dropped entry.
    pub dropped: Vec<String>,
}

#[derive(Deserialize)]
struct RawIndex {
    schema: u32,
    #[serde(default)]
    repo: Option<RepoMeta>,
    #[serde(default)]
    plugins: Vec<serde_json::Value>,
}

impl RepoIndex {
    /// Read an index. An entry that does not parse or validate is dropped and
    /// named in [`Parsed::dropped`]; the repository as a whole is refused only
    /// for an unknown format or an id listed twice.
    pub fn parse(bytes: &[u8]) -> Result<Parsed, RepoError> {
        if bytes.len() > MAX_INDEX_BYTES {
            return Err(RepoError::Unreadable(format!(
                "it is larger than {MAX_INDEX_BYTES} bytes"
            )));
        }
        let raw: RawIndex = serde_json::from_slice(bytes)
            .map_err(|error| RepoError::Unreadable(error.to_string()))?;
        if raw.schema != REPO_SCHEMA {
            return Err(RepoError::Schema {
                theirs: raw.schema,
                ours: REPO_SCHEMA,
            });
        }
        let mut dropped = Vec::new();
        let mut seen = BTreeSet::new();
        let mut plugins = Vec::new();
        for (position, value) in raw.plugins.into_iter().enumerate() {
            let plugin: RepoPlugin = match serde_json::from_value(value) {
                Ok(plugin) => plugin,
                Err(error) => {
                    dropped.push(format!("entry {}: {error}", position + 1));
                    continue;
                }
            };
            if let Some(problem) = plugin.problem() {
                dropped.push(problem);
                continue;
            }
            if !seen.insert(plugin.id.clone()) {
                return Err(RepoError::Duplicate(plugin.id));
            }
            plugins.push(plugin);
        }
        Ok(Parsed {
            index: RepoIndex {
                schema: raw.schema,
                repo: raw.repo.unwrap_or_default(),
                plugins,
            },
            dropped,
        })
    }

    pub fn plugin(&self, id: &str) -> Option<&RepoPlugin> {
        self.plugins.iter().find(|plugin| plugin.id == id)
    }
}

/// One repository on the vetted list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VettedRepo {
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub maintainer: String,
}

/// `repositories.min.json`: the third-party repositories flexdeck has reviewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VettedList {
    pub schema: u32,
    #[serde(default)]
    pub repositories: Vec<VettedRepo>,
}

impl VettedList {
    pub fn parse(bytes: &[u8]) -> Result<Self, RepoError> {
        if bytes.len() > MAX_INDEX_BYTES {
            return Err(RepoError::Unreadable(format!(
                "it is larger than {MAX_INDEX_BYTES} bytes"
            )));
        }
        let list: Self = serde_json::from_slice(bytes)
            .map_err(|error| RepoError::Unreadable(error.to_string()))?;
        if list.schema != REPO_SCHEMA {
            return Err(RepoError::Schema {
                theirs: list.schema,
                ours: REPO_SCHEMA,
            });
        }
        Ok(list)
    }
}

/// Compare versions so `1.10.0` sorts above `1.9.0`: numeric where both parts
/// are numbers, and a release above its own pre-release text.
pub fn version_order(left: &str, right: &str) -> std::cmp::Ordering {
    let parts = |text: &str| -> Vec<Result<u64, String>> {
        text.split(['.', '-', '+'])
            .map(|part| part.parse::<u64>().map_err(|_| part.to_string()))
            .collect()
    };
    let (left, right) = (parts(left), parts(right));
    for (a, b) in left.iter().zip(right.iter()) {
        let ordering = match (a, b) {
            (Ok(a), Ok(b)) => a.cmp(b),
            (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
            (Err(_), Ok(_)) => std::cmp::Ordering::Less,
            (Err(a), Err(b)) => a.cmp(b),
        };
        if ordering.is_ne() {
            return ordering;
        }
    }
    // `1.0.0-beta` is before `1.0.0`; `1.0.0.1` is after it.
    let (longer, shorter, flip) = if left.len() >= right.len() {
        (&left, &right, false)
    } else {
        (&right, &left, true)
    };
    let ordering = match longer.get(shorter.len()) {
        None => std::cmp::Ordering::Equal,
        Some(Ok(_)) => std::cmp::Ordering::Greater,
        Some(Err(_)) => std::cmp::Ordering::Less,
    };
    if flip {
        ordering.reverse()
    } else {
        ordering
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn entry(id: &str, version: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": id, "version": version, "abi": 2,
            "package": { "url": format!("{id}.fdpkg"), "sha256": SHA }
        })
    }

    fn index(plugins: Vec<serde_json::Value>) -> Vec<u8> {
        serde_json::json!({ "schema": 1, "repo": { "name": "test" }, "plugins": plugins })
            .to_string()
            .into_bytes()
    }

    #[test]
    fn a_bad_entry_is_dropped_and_the_rest_still_list() {
        let parsed = RepoIndex::parse(&index(vec![
            entry("obs", "1.0.0"),
            entry("Bad Id", "1.0.0"),
            entry("lights", "../x"),
            serde_json::json!({ "id": "nameless" }),
        ]))
        .unwrap();
        assert_eq!(parsed.index.plugins.len(), 1);
        assert_eq!(parsed.index.plugins[0].id, "obs");
        assert_eq!(parsed.dropped.len(), 3, "{:?}", parsed.dropped);
    }

    #[test]
    fn a_checksum_that_is_not_sha256_is_dropped() {
        let mut bad = entry("obs", "1.0.0");
        bad["package"]["sha256"] = "abc".into();
        assert!(RepoIndex::parse(&index(vec![bad]))
            .unwrap()
            .index
            .plugins
            .is_empty());
    }

    #[test]
    fn an_id_listed_twice_refuses_the_repository() {
        let error = RepoIndex::parse(&index(vec![entry("obs", "1.0.0"), entry("obs", "2.0.0")]))
            .unwrap_err();
        assert_eq!(error, RepoError::Duplicate("obs".into()));
    }

    #[test]
    fn an_unknown_format_is_refused_with_both_numbers() {
        let error = RepoIndex::parse(br#"{"schema": 2, "repo": {"name": "x"}}"#).unwrap_err();
        assert_eq!(error, RepoError::Schema { theirs: 2, ours: 1 });
        assert!(RepoIndex::parse(b"<html>").is_err());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let mut extra = entry("obs", "1.0.0");
        extra["screenshots"] = serde_json::json!(["a.png"]);
        let bytes = serde_json::json!({
            "schema": 1, "repo": { "name": "x", "mirror": "y" }, "plugins": [extra], "future": true
        });
        let parsed = RepoIndex::parse(bytes.to_string().as_bytes()).unwrap();
        assert_eq!(parsed.index.plugins.len(), 1);
        assert!(parsed.dropped.is_empty());
    }

    #[test]
    fn the_icon_closest_to_the_size_wins() {
        let mut plugin: RepoPlugin = serde_json::from_value(entry("obs", "1.0.0")).unwrap();
        assert_eq!(plugin.icon_for(64), None);
        plugin.icon = Some("main.png".into());
        assert_eq!(plugin.icon_for(64), Some("main.png"));
        plugin.icons.insert("32".into(), "32.png".into());
        plugin.icons.insert("128".into(), "128.png".into());
        plugin.icons.insert("256".into(), "256.png".into());
        assert_eq!(plugin.icon_for(64), Some("128.png"));
        assert_eq!(plugin.icon_for(512), Some("256.png"));
    }

    #[test]
    fn versions_order_numerically() {
        assert_eq!(version_order("1.10.0", "1.9.0"), Ordering::Greater);
        assert_eq!(version_order("1.0.0", "1.0.0"), Ordering::Equal);
        assert_eq!(version_order("1.0.0", "1.0.0-beta"), Ordering::Greater);
        assert_eq!(version_order("1.0.0-beta", "1.0.0"), Ordering::Less);
        assert_eq!(version_order("1.0.0.1", "1.0.0"), Ordering::Greater);
        assert_eq!(version_order("0.2.0", "0.10.0"), Ordering::Less);
    }

    #[test]
    fn the_contract_fixture_parses_cleanly() {
        let parsed = RepoIndex::parse(include_bytes!(
            "../../../contract/fixtures/plugin/repository.min.json"
        ))
        .unwrap();
        assert!(parsed.dropped.is_empty(), "{:?}", parsed.dropped);
        let sample = parsed
            .index
            .plugin("sample")
            .expect("the sample plugin is listed");
        assert_eq!(sample.version, "1.2.3");
        assert_eq!(sample.package.url, "sample.fdpkg");
    }

    #[test]
    fn the_vetted_list_reads() {
        let list = VettedList::parse(
            br#"{"schema":1,"repositories":[{"url":"https://a.example/repository.min.json","name":"A"}]}"#,
        )
        .unwrap();
        assert_eq!(list.repositories[0].name, "A");
    }
}
