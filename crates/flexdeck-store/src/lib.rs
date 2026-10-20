//! SQLite-backed store for the flexdeck agent.
//!
//! The store is **schema-agnostic**: it deals in `(name, rev) -> JSON` documents,
//! not in `Config` structs. That keeps it decoupled from the proto contract and
//! means the same store serves every named document — today a configuration and
//! an automations document, tomorrow a third — through one implementation of
//! the only invariant that matters: revision numbers are monotonically increasing
//! per document and are never reused.
//!
//! # Why per-document heads, not one global head
//!
//! The configuration and the automations document change for different reasons,
//! at different rates, and at different costs. Sharing one head number between
//! them — say, putting both rev numbers on the same broadcast — would let an
//! automation revision back up to a configuration revision or skip forward, and
//! the two timelines could silently diverge. Each named document has its own
//! head pointer in `meta` and its own row in `revision`, and the two counters
//! are never combined.
//!
//! # Head pointer, not truncation
//!
//! `meta.<doc>.head_rev` is the *current* revision and is not necessarily the
//! newest row. Undo moves it backwards without deleting anything, so redo
//! works. Committing while rewound discards the abandoned branch — the
//! behaviour every editor has.
//!
//! # Reserved keys
//!
//! `set_meta` refuses any name a document has registered as a head, so a stray
//! generic settings write cannot rewind or skip a document's timeline. The
//! guard is `is_reserved_key`, the single source of truth for the set of
//! reserved names.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("invalid configuration JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("no configuration has been committed yet")]
    NoConfiguration,

    #[error("cannot undo: already at the oldest revision")]
    NothingToUndo,

    #[error("cannot redo: already at the newest revision")]
    NothingToRedo,

    #[error("unknown revision {0}")]
    UnknownRevision(u64),

    #[error("'{0}' is reserved for the store's own use")]
    ReservedKey(String),
}

pub type StoreResult<T> = std::result::Result<T, StoreError>;

/// A client that has been paired with the agent.
///
/// Only the *hash* of the device token is stored, so a stolen database file
/// does not hand over live credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub kind: DeviceKind,
}

/// What a paired credential is, which decides where it is accepted and what it
/// may do. Stored as text so a row reads plainly in a database browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DeviceKind {
    /// A tablet paired over the LAN: reads the deck and presses buttons.
    #[serde(rename = "tablet")]
    Tablet,
    /// A browser on this very computer, connected through the loopback listener.
    #[serde(rename = "browser-local")]
    BrowserLocal,
    /// A browser elsewhere, paired with a code and reaching in through the cloud.
    #[serde(rename = "browser-remote")]
    BrowserRemote,
    /// A coding agent on this computer, reaching the MCP endpoint through the
    /// loopback listener.
    #[serde(rename = "agent")]
    Agent,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tablet => "tablet",
            Self::BrowserLocal => "browser-local",
            Self::BrowserRemote => "browser-remote",
            Self::Agent => "agent",
        }
    }

    /// Unknown text reads as the least-privileged kind, never a more powerful one.
    fn from_column(text: &str) -> Self {
        match text {
            "browser-local" => Self::BrowserLocal,
            "browser-remote" => Self::BrowserRemote,
            "agent" => Self::Agent,
            _ => Self::Tablet,
        }
    }
}

/// A point in a document's timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Revision {
    pub rev: u64,
    pub created_at: i64,
}

/// A named document kept in the same SQLite database.
///
/// Two documents share a table (`revision`) and a metadata table (`meta`), but
/// have **independent** timelines and **independent** head pointers. The
/// `name` is the row partition in `revision`; `head_key` is the meta key under
/// which this document's head revision lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Document {
    pub name: &'static str,
    pub head_key: &'static str,
}

pub const CONFIG: Document = Document {
    name: "config",
    head_key: "head_rev",
};
pub const AUTOMATIONS: Document = Document {
    name: "automations",
    head_key: "automations_head_rev",
};

/// The historical alias for the config head key, kept so a client that still
/// inspects `meta("head_rev")` continues to find it.
pub const HEAD_REV_KEY: &str = CONFIG.head_key;

/// Whether a meta key is one of the documents' head pointers and must not be
/// written directly. The single source of truth for the set of reserved names.
pub fn is_reserved_key(key: &str) -> bool {
    key == CONFIG.head_key || key == AUTOMATIONS.head_key
}

/// Schema for tables other than `revision`; `revision` is created by [`migrate`]
/// so an existing single-document database is upgraded in place rather than
/// recreated empty.
const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS device (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    revoked    INTEGER NOT NULL DEFAULT 0,
    kind       TEXT NOT NULL DEFAULT 'tablet',
    client     TEXT
);
"#;

pub struct ConfigStore {
    conn: Connection,
}

impl ConfigStore {
    /// Open (creating if needed) a store at the given path.
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// An in-memory store. Mostly useful in tests.
    pub fn open_in_memory() -> StoreResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> StoreResult<Self> {
        conn.execute_batch(SCHEMA_SQL)?;
        migrate(&conn)?;
        migrate_device_kind(&conn)?;
        migrate_device_client(&conn)?;
        Ok(Self { conn })
    }

    // -- generic revision API ------------------------------------------------

    /// Commit a new revision and make it current.
    ///
    /// Returns the new revision number, which is strictly greater than every
    /// revision **this document** has ever issued. Numbers are never reused, so
    /// a revision number always identifies exactly one document.
    ///
    /// Committing while rewound discards any revisions after the current head —
    /// the timeline has branched and the abandoned future is no longer
    /// reachable. Its numbers are not handed out again.
    ///
    /// This is the only place the per-document `max + 1` invariant is enforced.
    /// Every other method that touches a document's timeline goes through it.
    pub fn commit_document(&self, doc: &Document, json: &str) -> StoreResult<u64> {
        // Validate before touching the database, so a malformed document
        // cannot leave the timeline half-written.
        let _: serde_json::Value = serde_json::from_str(json)?;

        let head = self.head_rev_for(doc)?.unwrap_or(0);
        // `max + 1`, never `head + 1`. Reusing a number after an undo would mean
        // two different documents were both called "rev 3", and a client still
        // holding the discarded one would be told it was up to date while
        // holding different content. That is silent divergence — the worst
        // failure this system can have — so numbers are spent, not recycled.
        let next = self.max_rev_for(doc)?.unwrap_or(0) + 1;
        let created_at = now_secs();

        let tx = self.conn.unchecked_transaction()?;
        // Committing while rewound abandons the future that was there.
        tx.execute(
            "DELETE FROM revision WHERE name = ?1 AND rev > ?2",
            params![doc.name, head],
        )?;
        tx.execute(
            "INSERT INTO revision (name, rev, json, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![doc.name, next, json, created_at],
        )?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![doc.head_key, next.to_string()],
        )?;
        tx.commit()?;

        Ok(next as u64)
    }

    /// Commit `json` only if the document is empty. Used to seed a first-run
    /// default without clobbering an existing document.
    pub fn seed_if_empty_document(&self, doc: &Document, json: &str) -> StoreResult<Option<u64>> {
        if self.head_rev_for(doc)?.is_some() {
            return Ok(None);
        }
        Ok(Some(self.commit_document(doc, json)?))
    }

    /// The revision [`Self::commit_document`] would produce next.
    ///
    /// Exposed so a caller can stamp a document *before* committing it. Two
    /// callers racing would both predict the same number, so this is only safe
    /// while one writer owns the store — which is how the agent uses it.
    pub fn next_rev_document(&self, doc: &Document) -> StoreResult<u64> {
        Ok(self.max_rev_for(doc)?.unwrap_or(0) as u64 + 1)
    }

    /// The current revision of `doc`, or `None` if nothing has been committed.
    pub fn rev_document(&self, doc: &Document) -> StoreResult<Option<u64>> {
        Ok(self.head_rev_for(doc)?.map(|rev| rev as u64))
    }

    /// The current `(rev, json)` pair of `doc`, or `None` if empty.
    pub fn snapshot_document(&self, doc: &Document) -> StoreResult<Option<(u64, String)>> {
        let Some(rev) = self.head_rev_for(doc)? else {
            return Ok(None);
        };
        let json = self.snapshot_at_document(doc, rev as u64)?;
        Ok(json.map(|json| (rev as u64, json)))
    }

    /// The JSON of a specific revision of `doc`, if it exists.
    pub fn snapshot_at_document(&self, doc: &Document, rev: u64) -> StoreResult<Option<String>> {
        let json = self
            .conn
            .query_row(
                "SELECT json FROM revision WHERE name = ?1 AND rev = ?2",
                params![doc.name, rev as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(json)
    }

    /// Every revision of `doc`, oldest first.
    pub fn revisions_document(&self, doc: &Document) -> StoreResult<Vec<Revision>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rev, created_at FROM revision WHERE name = ?1 ORDER BY rev ASC")?;
        let rows = stmt.query_map(params![doc.name], |row| {
            Ok(Revision {
                rev: row.get::<_, i64>(0)? as u64,
                created_at: row.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// How many revisions `doc` holds, including abandoned branches.
    pub fn history_len_document(&self, doc: &Document) -> StoreResult<u64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM revision WHERE name = ?1",
            params![doc.name],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Step `doc`'s head pointer back one revision. Nothing is deleted, so the
    /// undone revision is still reachable by [`Self::redo_document`].
    pub fn undo_document(&self, doc: &Document) -> StoreResult<(u64, String)> {
        let head = self.head_rev_for(doc)?.ok_or(StoreError::NoConfiguration)?;
        let previous = self
            .neighbour_rev_for(doc, head, Step::Back)?
            .ok_or(StoreError::NothingToUndo)?;
        let json = self
            .snapshot_at_document(doc, previous as u64)?
            .ok_or(StoreError::NothingToUndo)?;
        self.set_head_for(doc, previous)?;
        Ok((previous as u64, json))
    }

    /// Step `doc`'s head pointer forward one revision, if a redo history exists.
    pub fn redo_document(&self, doc: &Document) -> StoreResult<(u64, String)> {
        let head = self.head_rev_for(doc)?.ok_or(StoreError::NoConfiguration)?;
        let Some(next) = self.neighbour_rev_for(doc, head, Step::Forward)? else {
            return Err(StoreError::NothingToRedo);
        };
        let Some(json) = self.snapshot_at_document(doc, next as u64)? else {
            return Err(StoreError::NothingToRedo);
        };
        self.set_head_for(doc, next)?;
        Ok((next as u64, json))
    }

    pub fn can_undo_document(&self, doc: &Document) -> StoreResult<bool> {
        let Some(head) = self.head_rev_for(doc)? else {
            return Ok(false);
        };
        Ok(self.neighbour_rev_for(doc, head, Step::Back)?.is_some())
    }

    pub fn can_redo_document(&self, doc: &Document) -> StoreResult<bool> {
        let Some(head) = self.head_rev_for(doc)? else {
            return Ok(false);
        };
        Ok(self.neighbour_rev_for(doc, head, Step::Forward)?.is_some())
    }

    pub fn undo_target_document(&self, doc: &Document) -> StoreResult<Option<u64>> {
        let Some(head) = self.head_rev_for(doc)? else {
            return Ok(None);
        };
        Ok(self.neighbour_rev_for(doc, head, Step::Back)?.map(|rev| rev as u64))
    }

    pub fn redo_target_document(&self, doc: &Document) -> StoreResult<Option<u64>> {
        let Some(head) = self.head_rev_for(doc)? else {
            return Ok(None);
        };
        Ok(self.neighbour_rev_for(doc, head, Step::Forward)?.map(|rev| rev as u64))
    }

    /// An RFC 6902 JSON-Patch that turns `from` into `to` for `doc`.
    pub fn diff_document(&self, doc: &Document, from: u64, to: u64) -> StoreResult<String> {
        let before = self
            .snapshot_at_document(doc, from)?
            .ok_or(StoreError::UnknownRevision(from))?;
        let after = self
            .snapshot_at_document(doc, to)?
            .ok_or(StoreError::UnknownRevision(to))?;

        let before: serde_json::Value = serde_json::from_str(&before)?;
        let after: serde_json::Value = serde_json::from_str(&after)?;

        let patch = json_patch::diff(&before, &after);
        Ok(serde_json::to_string(&patch)?)
    }

    // -- config-only wrappers (existing API preserved) -----------------------

    pub fn commit(&self, json: &str) -> StoreResult<u64> {
        self.commit_document(&CONFIG, json)
    }

    pub fn seed_if_empty(&self, json: &str) -> StoreResult<Option<u64>> {
        self.seed_if_empty_document(&CONFIG, json)
    }

    pub fn rev(&self) -> StoreResult<Option<u64>> {
        self.rev_document(&CONFIG)
    }

    pub fn snapshot(&self) -> StoreResult<Option<(u64, String)>> {
        self.snapshot_document(&CONFIG)
    }

    pub fn snapshot_at(&self, rev: u64) -> StoreResult<Option<String>> {
        self.snapshot_at_document(&CONFIG, rev)
    }

    pub fn revisions(&self) -> StoreResult<Vec<Revision>> {
        self.revisions_document(&CONFIG)
    }

    pub fn history_len(&self) -> StoreResult<u64> {
        self.history_len_document(&CONFIG)
    }

    pub fn next_rev(&self) -> StoreResult<u64> {
        self.next_rev_document(&CONFIG)
    }

    pub fn undo(&self) -> StoreResult<(u64, String)> {
        self.undo_document(&CONFIG)
    }

    pub fn redo(&self) -> StoreResult<(u64, String)> {
        self.redo_document(&CONFIG)
    }

    pub fn can_undo(&self) -> StoreResult<bool> {
        self.can_undo_document(&CONFIG)
    }

    pub fn can_redo(&self) -> StoreResult<bool> {
        self.can_redo_document(&CONFIG)
    }

    pub fn diff(&self, from: u64, to: u64) -> StoreResult<String> {
        self.diff_document(&CONFIG, from, to)
    }

    // -- metadata -------------------------------------------------------------

    /// Read a value from the metadata table.
    ///
    /// The store does not interpret these values; callers own their own keys.
    pub fn meta(&self, key: &str) -> StoreResult<Option<String>> {
        let value = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(value)
    }

    /// Write a value to the metadata table.
    ///
    /// Refuses every reserved head pointer. A reserved name that the guard does
    /// not know about would let a stray settings write silently rewind a
    /// document's timeline — the exact bug `HEAD_REV_KEY` was created to
    /// prevent.
    pub fn set_meta(&self, key: &str, value: &str) -> StoreResult<()> {
        if is_reserved_key(key) {
            return Err(StoreError::ReservedKey(key.to_string()));
        }

        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // -- devices -------------------------------------------------------------

    /// Register a paired tablet, storing only the hash of its token.
    pub fn add_device(&self, id: &str, name: &str, token_hash: &str) -> StoreResult<()> {
        self.add_device_of_kind(id, name, token_hash, DeviceKind::Tablet)
    }

    /// Register a paired credential of any kind, storing only its hash.
    pub fn add_device_of_kind(
        &self,
        id: &str,
        name: &str,
        token_hash: &str,
        kind: DeviceKind,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO device (id, name, token_hash, created_at, revoked, kind)
             VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![id, name, token_hash, now_secs(), kind.as_str()],
        )?;
        Ok(())
    }

    /// Register a credential for a client that says who it is, revoking any
    /// earlier credential the same client was given. A tablet re-paired, or a
    /// browser that connected again, replaces its old row rather than piling
    /// up beside it. Returns how many old rows were revoked.
    ///
    /// `client` is an id the client made up and keeps — not a secret, and not
    /// proof of anything: a client claiming another's id only revokes that
    /// one, and it already holds a working credential of its own to do that.
    pub fn add_device_for_client(
        &self,
        id: &str,
        name: &str,
        token_hash: &str,
        kind: DeviceKind,
        client: &str,
    ) -> StoreResult<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let revoked = tx.execute(
            "UPDATE device SET revoked = 1 WHERE client = ?1 AND kind = ?2 AND revoked = 0",
            params![client, kind.as_str()],
        )?;
        tx.execute(
            "INSERT INTO device (id, name, token_hash, created_at, revoked, kind, client)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
            params![id, name, token_hash, now_secs(), kind.as_str(), client],
        )?;
        tx.commit()?;
        Ok(revoked)
    }

    /// Look up a live device by the hash of the token it presented.
    ///
    /// Returns `None` for unknown *and* for revoked devices, so a revoked
    /// device is indistinguishable from a never-paired one.
    pub fn authenticate(&self, token_hash: &str) -> StoreResult<Option<Device>> {
        let device = self
            .conn
            .query_row(
                "SELECT id, name, created_at, kind FROM device
                 WHERE token_hash = ?1 AND revoked = 0",
                params![token_hash],
                device_from_row,
            )
            .optional()?;
        Ok(device)
    }

    /// All live paired devices.
    ///
    /// Revoked rows are left out: `Device` cannot say it is revoked, so listing
    /// one shows a device that looks paired, can no longer connect, and refuses
    /// a second Remove as "not on the list".
    pub fn devices(&self) -> StoreResult<Vec<Device>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, created_at, kind FROM device
             WHERE revoked = 0 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], device_from_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Revoke a device. Returns `true` if a live device was actually revoked.
    pub fn revoke_device(&self, id: &str) -> StoreResult<bool> {
        let changed = self.conn.execute(
            "UPDATE device SET revoked = 1 WHERE id = ?1 AND revoked = 0",
            params![id],
        )?;
        Ok(changed > 0)
    }

    /// Revoke every live device except `keep` (the one asking, so it is not
    /// locked out by its own request). Returns how many were revoked.
    pub fn revoke_all_devices(&self, keep: Option<&str>) -> StoreResult<usize> {
        let changed = self.conn.execute(
            "UPDATE device SET revoked = 1 WHERE revoked = 0 AND id IS NOT ?1 AND kind IS NOT ?2",
            params![keep, DeviceKind::Agent.as_str()],
        )?;
        Ok(changed)
    }

    pub fn rename_device(&self, id: &str, name: &str) -> StoreResult<bool> {
        let changed = self.conn.execute(
            "UPDATE device SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(changed > 0)
    }

    // -- internals -----------------------------------------------------------

    fn head_rev_for(&self, doc: &Document) -> StoreResult<Option<i64>> {
        let raw = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![doc.head_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(raw.and_then(|text| text.parse::<i64>().ok()))
    }

    /// The highest revision number this document has ever issued.
    ///
    /// Deliberately not derivable from `head_rev_for`, which can be rewound
    /// past it. `SELECT MAX(rev) WHERE name = ?` returns `NULL` for an empty
    /// partition, hence the nested `Option`.
    fn max_rev_for(&self, doc: &Document) -> StoreResult<Option<i64>> {
        let highest = self.conn.query_row(
            "SELECT MAX(rev) FROM revision WHERE name = ?1",
            params![doc.name],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        Ok(highest)
    }

    fn neighbour_rev_for(&self, doc: &Document, head: i64, step: Step) -> StoreResult<Option<i64>> {
        let sql = match step {
            Step::Back => "SELECT MAX(rev) FROM revision WHERE name = ?1 AND rev < ?2",
            Step::Forward => "SELECT MIN(rev) FROM revision WHERE name = ?1 AND rev > ?2",
        };
        let rev = self
            .conn
            .query_row(sql, params![doc.name, head], |row| row.get::<_, Option<i64>>(0))?;
        Ok(rev)
    }

    fn set_head_for(&self, doc: &Document, rev: i64) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![doc.head_key, rev.to_string()],
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Step {
    Back,
    Forward,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// Bring an existing single-document database up to the named-documents shape.
///
/// Before the generalisation, `revision` had `rev` as its primary key and every
/// row belonged to the configuration. The new table adds a `name` column and
/// makes `(name, rev)` the primary key, so two documents can each have a `1`
/// without colliding. Existing rows are preserved under `name = 'config'`.
fn device_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Device> {
    let kind: String = row.get(3)?;
    Ok(Device {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        kind: DeviceKind::from_column(&kind),
    })
}

/// Give a database from before credential kinds its `kind` column. Every row
/// that existed then was a tablet, which is exactly what the default says.
fn migrate_device_kind(conn: &Connection) -> rusqlite::Result<()> {
    let has_kind: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('device') WHERE name = 'kind')",
        [],
        |row| row.get(0),
    )?;
    if !has_kind {
        conn.execute_batch("ALTER TABLE device ADD COLUMN kind TEXT NOT NULL DEFAULT 'tablet';")?;
    }
    Ok(())
}

/// Give a database from before client ids its `client` column. Old rows have
/// none, so they are never matched — "Remove all" is how those are cleared.
fn migrate_device_client(conn: &Connection) -> rusqlite::Result<()> {
    let has_client: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('device') WHERE name = 'client')",
        [],
        |row| row.get(0),
    )?;
    if !has_client {
        conn.execute_batch("ALTER TABLE device ADD COLUMN client TEXT;")?;
    }
    Ok(())
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let has_name: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('revision') WHERE name = 'name')",
        [],
        |row| row.get(0),
    )?;
    if has_name {
        return Ok(());
    }

    let has_table: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'revision')",
        [],
        |row| row.get(0),
    )?;

    if has_table {
        conn.execute_batch(
            "ALTER TABLE revision RENAME TO revision_v0;
             CREATE TABLE revision (
                 name       TEXT    NOT NULL,
                 rev        INTEGER NOT NULL,
                 json       TEXT    NOT NULL,
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY (name, rev)
             );
             INSERT INTO revision (name, rev, json, created_at)
                 SELECT 'config', rev, json, created_at FROM revision_v0;
             CREATE INDEX revision_name_idx ON revision(name);
             DROP TABLE revision_v0;",
        )?;
    } else {
        conn.execute_batch(
            "CREATE TABLE revision (
                 name       TEXT    NOT NULL,
                 rev        INTEGER NOT NULL,
                 json       TEXT    NOT NULL,
                 created_at INTEGER NOT NULL,
                 PRIMARY KEY (name, rev)
             );
             CREATE INDEX revision_name_idx ON revision(name);",
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ConfigStore {
        ConfigStore::open_in_memory().expect("in-memory store opens")
    }

    // -- existing config tests (preserved verbatim) -------------------------

    #[test]
    fn a_fresh_store_has_no_revision() {
        let store = store();
        assert_eq!(store.rev().unwrap(), None);
        assert_eq!(store.snapshot().unwrap(), None);
        assert!(!store.can_undo().unwrap());
        assert!(!store.can_redo().unwrap());
    }

    #[test]
    fn commit_advances_the_revision_and_returns_it() {
        let store = store();
        assert_eq!(store.commit("{\"rev\":1}").unwrap(), 1);
        assert_eq!(store.commit("{\"rev\":2}").unwrap(), 2);
        assert_eq!(store.rev().unwrap(), Some(2));
        assert_eq!(
            store.snapshot().unwrap(),
            Some((2, "{\"rev\":2}".to_string()))
        );
    }

    #[test]
    fn invalid_json_is_rejected_without_advancing_the_timeline() {
        let store = store();
        store.commit("{\"a\":1}").unwrap();

        let err = store.commit("{not json").unwrap_err();
        assert!(matches!(err, StoreError::InvalidJson(_)));
        assert_eq!(store.rev().unwrap(), Some(1));
        assert_eq!(store.history_len().unwrap(), 1);
    }

    #[test]
    fn seed_if_empty_only_applies_once() {
        let store = store();
        assert_eq!(store.seed_if_empty("{\"seed\":true}").unwrap(), Some(1));
        assert_eq!(store.seed_if_empty("{\"seed\":false}").unwrap(), None);
        assert_eq!(store.snapshot().unwrap().unwrap().1, "{\"seed\":true}");
    }

    #[test]
    fn undo_and_redo_step_over_the_numbers_an_abandoned_branch_used() {
        let store = store();
        store.commit("{\"step\":1}").unwrap();
        store.commit("{\"step\":2}").unwrap();
        store.commit("{\"step\":3}").unwrap();
        store.undo().unwrap();
        assert_eq!(store.commit("{\"step\":4}").unwrap(), 4);

        assert!(store.can_undo().unwrap());
        assert_eq!(store.undo_target_document(&CONFIG).unwrap(), Some(2));
        assert_eq!(store.undo().unwrap(), (2, "{\"step\":2}".to_string()));
        assert_eq!(store.undo().unwrap(), (1, "{\"step\":1}".to_string()));
        assert!(!store.can_undo().unwrap());
        assert_eq!(store.redo().unwrap(), (2, "{\"step\":2}".to_string()));
        assert!(store.can_redo().unwrap());
        assert_eq!(store.redo_target_document(&CONFIG).unwrap(), Some(4));
        assert_eq!(store.redo().unwrap(), (4, "{\"step\":4}".to_string()));
        assert!(!store.can_redo().unwrap());
    }

    #[test]
    fn an_agent_row_reads_back_as_an_agent() {
        let store = store();
        store.add_device_of_kind("a1", "Claude", "h", DeviceKind::Agent).unwrap();
        assert_eq!(store.authenticate("h").unwrap().unwrap().kind, DeviceKind::Agent);
    }

    #[test]
    fn undo_and_redo_walk_the_timeline_without_deleting() {
        let store = store();
        store.commit("{\"step\":1}").unwrap();
        store.commit("{\"step\":2}").unwrap();
        store.commit("{\"step\":3}").unwrap();

        assert_eq!(store.undo().unwrap(), (2, "{\"step\":2}".to_string()));
        assert_eq!(store.undo().unwrap(), (1, "{\"step\":1}".to_string()));
        assert!(!store.can_undo().unwrap());
        assert_eq!(
            store.undo().unwrap_err().to_string(),
            "cannot undo: already at the oldest revision"
        );

        assert_eq!(store.redo().unwrap(), (2, "{\"step\":2}".to_string()));
        assert_eq!(store.redo().unwrap(), (3, "{\"step\":3}".to_string()));
        assert!(!store.can_redo().unwrap());

        assert_eq!(store.history_len().unwrap(), 3);
    }

    #[test]
    fn committed_while_rewound_discards_the_abandoned_branch() {
        let store = store();
        store.commit("{\"step\":1}").unwrap();
        store.commit("{\"step\":2}").unwrap();
        store.commit("{\"step\":3}").unwrap();

        store.undo().unwrap(); // now at rev 2

        let branched = store.commit(r#"{"step":"2b"}"#).unwrap();

        assert_eq!(branched, 4);
        assert_eq!(
            store.snapshot().unwrap(),
            Some((4, r#"{"step":"2b"}"#.to_string()))
        );
        assert!(!store.can_redo().unwrap());
        assert_eq!(store.snapshot_at(3).unwrap(), None, "rev 3 was discarded");
        assert_eq!(store.snapshot_at(4).unwrap().unwrap(), r#"{"step":"2b"}"#);
    }

    #[test]
    fn revision_numbers_are_never_reused() {
        let store = store();
        store.commit("{\"a\":1}").unwrap();
        store.commit("{\"a\":2}").unwrap();

        store.undo().unwrap();
        let branched = store.commit("{\"a\":3}").unwrap();
        assert_eq!(branched, 3, "skips the discarded 2 rather than reusing it");

        store.commit("{\"a\":4}").unwrap();

        let issued: Vec<u64> = store
            .revisions()
            .unwrap()
            .into_iter()
            .map(|revision| revision.rev)
            .collect();
        assert_eq!(issued, vec![1, 3, 4]);
    }

    #[test]
    fn next_rev_predicts_what_commit_produces() {
        let store = store();
        assert_eq!(store.next_rev().unwrap(), 1, "even on an empty store");

        assert_eq!(store.commit("{\"a\":1}").unwrap(), 1);
        assert_eq!(store.next_rev().unwrap(), 2);

        assert_eq!(store.commit("{\"a\":2}").unwrap(), 2);
        store.undo().unwrap();

        assert_eq!(store.next_rev().unwrap(), 3);
        assert_eq!(store.commit("{\"a\":3}").unwrap(), 3);
    }

    #[test]
    fn metadata_round_trips() {
        let store = store();
        assert_eq!(store.meta("greeting").unwrap(), None, "nothing stored yet");
        store.set_meta("greeting", "hello").unwrap();
        assert_eq!(store.meta("greeting").unwrap(), Some("hello".to_string()));

        store.set_meta("greeting", "goodbye").unwrap();
        assert_eq!(store.meta("greeting").unwrap(), Some("goodbye".to_string()));
    }

    #[test]
    fn the_revision_log_key_cannot_be_written_through_the_metadata_api() {
        let store = store();
        store.commit("{\"a\":1}").unwrap();

        let error = store.set_meta(HEAD_REV_KEY, "99").unwrap_err();

        assert!(matches!(error, StoreError::ReservedKey(_)));
        assert_eq!(store.rev().unwrap(), Some(1), "the timeline is untouched");
    }

    #[test]
    fn metadata_and_the_timeline_do_not_interfere() {
        let store = store();
        store.set_meta("settings", "{}").unwrap();
        store.commit("{\"a\":1}").unwrap();

        assert_eq!(store.meta("settings").unwrap(), Some("{}".to_string()));
        assert_eq!(store.rev().unwrap(), Some(1));
    }

    #[test]
    fn undo_supports_at_least_fifty_steps() {
        let store = store();
        for step in 1..=60 {
            store.commit(&format!("{{\"step\":{step}}}")).unwrap();
        }
        for expected in (1..=59).rev() {
            let (rev, _) = store.undo().unwrap();
            assert_eq!(rev, expected as u64);
        }
        assert_eq!(store.rev().unwrap(), Some(1));
    }

    #[test]
    fn diff_produces_a_patch_that_actually_transforms_from_into_to() {
        let store = store();
        store.commit("{\"a\":1,\"keep\":true}").unwrap();
        store.commit("{\"a\":2,\"b\":3,\"keep\":true}").unwrap();

        let patch = store.diff(1, 2).unwrap();

        let mut document: serde_json::Value =
            serde_json::from_str(&store.snapshot_at(1).unwrap().unwrap()).unwrap();
        let parsed: json_patch::Patch = serde_json::from_str(&patch).unwrap();
        json_patch::patch(&mut document, &parsed).unwrap();

        let target: serde_json::Value =
            serde_json::from_str(&store.snapshot_at(2).unwrap().unwrap()).unwrap();
        assert_eq!(document, target);
    }

    #[test]
    fn diff_against_an_unknown_revision_is_an_error() {
        let store = store();
        store.commit("{\"a\":1}").unwrap();
        assert!(matches!(
            store.diff(1, 99).unwrap_err(),
            StoreError::UnknownRevision(99)
        ));
    }

    #[test]
    fn devices_authenticate_by_token_hash_and_can_be_revoked() {
        let store = store();
        store
            .add_device("tab-1", "Kitchen tablet", "hash-aaa")
            .unwrap();
        store
            .add_device("tab-2", "Studio tablet", "hash-bbb")
            .unwrap();

        let found = store
            .authenticate("hash-aaa")
            .unwrap()
            .expect("device found");
        assert_eq!(found.id, "tab-1");
        assert_eq!(found.name, "Kitchen tablet");

        assert_eq!(store.authenticate("nope").unwrap(), None);

        assert!(store.revoke_device("tab-1").unwrap());
        assert_eq!(store.authenticate("hash-aaa").unwrap(), None);
        assert!(!store.revoke_device("tab-1").unwrap());
        let listed: Vec<_> = store.devices().unwrap().into_iter().map(|d| d.id).collect();
        assert_eq!(listed, ["tab-2"], "a revoked device is not listed");

        assert!(store.rename_device("tab-2", "Renamed").unwrap());
        assert_eq!(
            store.authenticate("hash-bbb").unwrap().unwrap().name,
            "Renamed"
        );
    }

    #[test]
    fn a_device_keeps_the_kind_it_was_paired_as() {
        let store = store();
        store.add_device("tab", "Tablet", "hash-t").unwrap();
        store
            .add_device_of_kind("web", "Laptop", "hash-w", DeviceKind::BrowserRemote)
            .unwrap();

        assert_eq!(
            store.authenticate("hash-t").unwrap().unwrap().kind,
            DeviceKind::Tablet
        );
        assert_eq!(
            store.authenticate("hash-w").unwrap().unwrap().kind,
            DeviceKind::BrowserRemote
        );
    }

    #[test]
    fn a_client_pairing_again_replaces_its_old_row() {
        let store = store();
        store
            .add_device_for_client("a", "Tablet", "hash-a", DeviceKind::Tablet, "install-1")
            .unwrap();
        store
            .add_device_for_client("b", "Other", "hash-b", DeviceKind::Tablet, "install-2")
            .unwrap();
        let revoked = store
            .add_device_for_client("c", "Tablet", "hash-c", DeviceKind::Tablet, "install-1")
            .unwrap();

        assert_eq!(revoked, 1);
        assert_eq!(store.authenticate("hash-a").unwrap(), None);
        let listed: Vec<_> = store.devices().unwrap().into_iter().map(|d| d.id).collect();
        assert_eq!(listed, ["b", "c"]);
    }

    #[test]
    fn revoking_all_keeps_the_one_asking() {
        let store = store();
        store.add_device("a", "A", "hash-a").unwrap();
        store.add_device("b", "B", "hash-b").unwrap();
        store.add_device("c", "C", "hash-c").unwrap();

        assert_eq!(store.revoke_all_devices(Some("b")).unwrap(), 2);
        let listed: Vec<_> = store.devices().unwrap().into_iter().map(|d| d.id).collect();
        assert_eq!(listed, ["b"]);
        assert_eq!(store.revoke_all_devices(None).unwrap(), 1);
        assert!(store.devices().unwrap().is_empty());
    }

    #[test]
    fn revoking_every_device_spares_coding_agents() {
        let store = ConfigStore::open_in_memory().unwrap();
        store.add_device_of_kind("t", "Tablet", "hash-t", DeviceKind::Tablet).unwrap();
        store.add_device_of_kind("c", "Claude", "hash-c", DeviceKind::Agent).unwrap();
        assert_eq!(store.revoke_all_devices(None).unwrap(), 1);
        let listed: Vec<_> = store.devices().unwrap().into_iter().map(|d| d.id).collect();
        assert_eq!(listed, ["c"]);
    }

    #[test]
    fn a_database_from_before_kinds_reads_its_devices_as_tablets() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE device (
                 id TEXT PRIMARY KEY, name TEXT NOT NULL,
                 token_hash TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL,
                 revoked INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO device VALUES ('old', 'Old tablet', 'hash-old', 1, 0);",
        )
        .unwrap();

        let store = ConfigStore::from_connection(conn).expect("migrates");
        assert_eq!(
            store.authenticate("hash-old").unwrap().unwrap().kind,
            DeviceKind::Tablet
        );
    }

    // -- new: named documents ------------------------------------------------

    #[test]
    fn the_reserved_key_set_is_a_single_source_of_truth() {
        assert!(is_reserved_key("head_rev"));
        assert!(is_reserved_key("automations_head_rev"));
        assert!(!is_reserved_key("settings"));
        assert!(!is_reserved_key(""));
    }

    #[test]
    fn the_automations_head_key_is_also_reserved() {
        let store = store();
        store
            .commit_document(&AUTOMATIONS, "{\"version\":1}")
            .unwrap();

        let error = store.set_meta(AUTOMATIONS.head_key, "99").unwrap_err();
        assert!(matches!(error, StoreError::ReservedKey(_)));
        assert_eq!(store.rev_document(&AUTOMATIONS).unwrap(), Some(1));
    }

    #[test]
    fn documents_share_a_database_but_have_independent_heads_and_revs() {
        let store = store();

        assert_eq!(store.rev_document(&CONFIG).unwrap(), None);
        assert_eq!(store.rev_document(&AUTOMATIONS).unwrap(), None);

        store.commit_document(&CONFIG, "{\"config\":1}").unwrap();
        store
            .commit_document(&AUTOMATIONS, "{\"automations\":1}")
            .unwrap();

        // Both documents start their numbering at 1.
        assert_eq!(store.rev_document(&CONFIG).unwrap(), Some(1));
        assert_eq!(store.rev_document(&AUTOMATIONS).unwrap(), Some(1));

        store.commit_document(&CONFIG, "{\"config\":2}").unwrap();

        // CONFIG is now at rev 2; AUTOMATIONS stayed at 1.
        assert_eq!(store.rev_document(&CONFIG).unwrap(), Some(2));
        assert_eq!(store.rev_document(&AUTOMATIONS).unwrap(), Some(1));
    }

    #[test]
    fn revisions_of_one_document_do_not_appear_in_the_other() {
        let store = store();
        store.commit_document(&CONFIG, "{\"c\":1}").unwrap();
        store.commit_document(&CONFIG, "{\"c\":2}").unwrap();
        store.commit_document(&AUTOMATIONS, "{\"a\":1}").unwrap();

        assert_eq!(
            store
                .revisions_document(&CONFIG)
                .unwrap()
                .into_iter()
                .map(|r| r.rev)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            store
                .revisions_document(&AUTOMATIONS)
                .unwrap()
                .into_iter()
                .map(|r| r.rev)
                .collect::<Vec<_>>(),
            vec![1]
        );

        assert!(store
            .snapshot_at_document(&AUTOMATIONS, 2)
            .unwrap()
            .is_none());
        assert!(store.snapshot_at_document(&CONFIG, 1).unwrap().is_some());
    }

    #[test]
    fn revision_numbers_are_per_document_and_independent() {
        let store = store();

        // Give CONFIG three commits, then undo the last one.
        store.commit_document(&CONFIG, "{\"c\":1}").unwrap();
        store.commit_document(&CONFIG, "{\"c\":2}").unwrap();
        store.commit_document(&CONFIG, "{\"c\":3}").unwrap();
        store.undo_document(&CONFIG).unwrap();

        // AUTOMATIONS must still get rev 1, not 3 — and CONFIG's next commit
        // must still skip the discarded number.
        assert_eq!(store.commit_document(&AUTOMATIONS, "{\"a\":1}").unwrap(), 1);
        assert_eq!(store.commit_document(&CONFIG, "{\"c\":\"3b\"}").unwrap(), 4);

        assert_eq!(
            store
                .revisions_document(&CONFIG)
                .unwrap()
                .into_iter()
                .map(|r| r.rev)
                .collect::<Vec<_>>(),
            vec![1, 2, 4]
        );
    }

    #[test]
    fn undo_and_redo_are_per_document() {
        let store = store();
        store.commit_document(&CONFIG, "{\"c\":1}").unwrap();
        store.commit_document(&CONFIG, "{\"c\":2}").unwrap();
        store.commit_document(&AUTOMATIONS, "{\"a\":1}").unwrap();
        store.commit_document(&AUTOMATIONS, "{\"a\":2}").unwrap();

        // Undoing CONFIG does not move AUTOMATIONS at all.
        assert_eq!(store.undo_document(&CONFIG).unwrap().0, 1);
        assert_eq!(store.rev_document(&CONFIG).unwrap(), Some(1));
        assert_eq!(store.rev_document(&AUTOMATIONS).unwrap(), Some(2));
        assert!(store.can_redo_document(&CONFIG).unwrap());
        assert!(!store.can_redo_document(&AUTOMATIONS).unwrap());
    }

    #[test]
    fn diff_is_per_document() {
        let store = store();
        store.commit_document(&CONFIG, "{\"c\":1}").unwrap();
        store.commit_document(&CONFIG, "{\"c\":2}").unwrap();
        store.commit_document(&AUTOMATIONS, "{\"a\":1}").unwrap();
        store.commit_document(&AUTOMATIONS, "{\"a\":2}").unwrap();

        let patch = store.diff_document(&CONFIG, 1, 2).unwrap();
        // The patch is JSON; its "path" is a JSON Pointer like "/c". Searching
        // for the bare key name in that form proves the diff is about `c`, not
        // `a`, without depending on patch internals.
        assert!(patch.contains("/c"));

        let other = store.diff_document(&AUTOMATIONS, 1, 2).unwrap();
        assert!(other.contains("/a"));
        assert!(!other.contains("/c"));
    }

    #[test]
    fn seed_if_empty_is_per_document() {
        let store = store();
        store
            .seed_if_empty_document(&CONFIG, "{\"c\":1}")
            .unwrap()
            .unwrap();

        assert!(store
            .seed_if_empty_document(&CONFIG, "{\"c\":2}")
            .unwrap()
            .is_none());

        // AUTOMATIONS is still empty, so its own seed applies.
        assert!(store
            .seed_if_empty_document(&AUTOMATIONS, "{\"a\":1}")
            .unwrap()
            .is_some());

        assert_eq!(store.rev_document(&CONFIG).unwrap(), Some(1));
        assert_eq!(store.rev_document(&AUTOMATIONS).unwrap(), Some(1));
    }

    // -- migration -----------------------------------------------------------

    #[test]
    fn migration_brings_an_old_single_document_database_forward() {
        // Open a database in the pre-generalisation shape, write to it, close.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("old.sqlite");
        {
            let store = ConfigStore::open(&path).expect("open");
            store.commit("{\"old\":1}").unwrap();
            store.commit("{\"old\":2}").unwrap();
            store.commit("{\"old\":3}").unwrap();
        }

        // Re-open through the migration.
        let store = ConfigStore::open(&path).expect("re-open");

        // The data survived and is now under the `config` partition.
        assert_eq!(store.rev_document(&CONFIG).unwrap(), Some(3));
        assert_eq!(
            store.snapshot_document(&CONFIG).unwrap(),
            Some((3, "{\"old\":3}".to_string()))
        );

        // And the new head keys exist alongside the old one.
        assert!(store.meta("head_rev").unwrap().is_some());
        assert!(store.meta("automations_head_rev").unwrap().is_none());
    }

    #[test]
    fn migration_is_idempotent_when_run_twice() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("idempotent.sqlite");
        {
            let store = ConfigStore::open(&path).expect("open");
            store.commit("{\"x\":1}").unwrap();
        }
        // First reopen already migrated; second reopen is a no-op.
        let _ = ConfigStore::open(&path).expect("first reopen");
        let _ = ConfigStore::open(&path).expect("second reopen");

        let store = ConfigStore::open(&path).expect("final");
        assert_eq!(store.rev_document(&CONFIG).unwrap(), Some(1));
    }
}
