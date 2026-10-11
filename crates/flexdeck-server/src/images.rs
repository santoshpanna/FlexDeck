//! Images plugins hand over — album art, avatars — for widgets to show.
//!
//! A widget names one as `img:<sha256>`. The bytes live here, in memory and
//! bounded: art changes with every track, and nothing about a picture of the
//! song that played an hour ago is worth keeping across a restart. Keyed by
//! content, so the same cover sent twice is stored once and a tablet can cache
//! it forever.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

/// The value a variable holds to name an image in this store.
pub const IMAGE_PREFIX: &str = "img:";

/// How many images to keep, and how many bytes in all, before the oldest go.
const MAX_ENTRIES: usize = 32;
const MAX_BYTES: usize = 8 * 1024 * 1024;

/// One stored image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub mime: String,
    pub bytes: Arc<[u8]>,
}

/// A small, content-addressed, least-recently-used image store.
#[derive(Debug, Default)]
pub struct ImageStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Most recently used last.
    entries: VecDeque<(String, Image)>,
    bytes: usize,
}

impl ImageStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep an image under its sha256 (lowercase hex). The caller has checked
    /// the hash; the store only bounds what it keeps.
    pub fn put(&self, sha256: &str, mime: &str, bytes: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = inner.entries.iter().position(|(key, _)| key == sha256) {
            let entry = inner.entries.remove(at).expect("found above");
            inner.entries.push_back(entry);
            return;
        }
        if bytes.len() > MAX_BYTES {
            return;
        }
        inner.bytes += bytes.len();
        inner.entries.push_back((
            sha256.to_string(),
            Image {
                mime: mime.to_string(),
                bytes: bytes.into(),
            },
        ));
        while inner.entries.len() > MAX_ENTRIES || inner.bytes > MAX_BYTES {
            let Some((_, oldest)) = inner.entries.pop_front() else {
                break;
            };
            inner.bytes -= oldest.bytes.len();
        }
    }

    /// The image stored under `sha256`, marking it recently used.
    pub fn get(&self, sha256: &str) -> Option<Image> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let at = inner.entries.iter().position(|(key, _)| key == sha256)?;
        let entry = inner.entries.remove(at).expect("found above");
        let image = entry.1.clone();
        inner.entries.push_back(entry);
        Some(image)
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The store, as the sink built-in plugins hand pictures to.
impl flexdeck_plugin::builtin::media_sessions::ImageSink for ImageStore {
    fn put(&self, mime: &str, bytes: Vec<u8>) -> String {
        use sha2::Digest as _;
        let sha256 = hex::encode(sha2::Sha256::digest(&bytes));
        ImageStore::put(self, &sha256, mime, bytes);
        format!("{IMAGE_PREFIX}{sha256}")
    }
}

/// Whether `sha256` is spelled like one: 64 lowercase hex digits. Checked before
/// a value from a URL is used as a key.
pub fn is_sha256(sha256: &str) -> bool {
    sha256.len() == 64 && sha256.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: usize) -> String {
        format!("{n:064x}")
    }

    #[test]
    fn an_image_comes_back_by_its_hash() {
        let store = ImageStore::new();
        store.put(&key(1), "image/png", vec![1, 2, 3]);
        let image = store.get(&key(1)).expect("stored");
        assert_eq!(image.mime, "image/png");
        assert_eq!(&*image.bytes, &[1, 2, 3]);
        assert!(store.get(&key(2)).is_none());
    }

    #[test]
    fn the_least_recently_used_goes_first() {
        let store = ImageStore::new();
        for n in 0..MAX_ENTRIES {
            store.put(&key(n), "image/png", vec![0]);
        }
        // Touch the oldest, so the second-oldest is the one to go.
        store.get(&key(0));
        store.put(&key(999), "image/png", vec![0]);
        assert_eq!(store.len(), MAX_ENTRIES);
        assert!(store.get(&key(0)).is_some(), "recently used survives");
        assert!(store.get(&key(1)).is_none(), "least recently used went");
    }

    #[test]
    fn the_total_size_is_bounded_too() {
        let store = ImageStore::new();
        let big = MAX_BYTES / 3 + 1;
        for n in 0..3 {
            store.put(&key(n), "image/jpeg", vec![0; big]);
        }
        assert_eq!(store.len(), 2, "three of them do not fit");
        assert!(store.get(&key(0)).is_none());
    }

    #[test]
    fn the_same_image_twice_is_kept_once() {
        let store = ImageStore::new();
        store.put(&key(1), "image/png", vec![1]);
        store.put(&key(1), "image/png", vec![1]);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn only_a_real_hash_spelling_is_a_key() {
        assert!(is_sha256(&key(7)));
        assert!(!is_sha256("../../etc/passwd"));
        assert!(is_sha256(&"ab".repeat(32)));
        assert!(!is_sha256(&"AB".repeat(32)));
        assert!(!is_sha256(&key(7)[1..]));
    }
}
