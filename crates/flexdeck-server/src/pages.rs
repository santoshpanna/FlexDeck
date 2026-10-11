//! "Go to page": from a step to every connected tablet.
//!
//! The deck plugin asks through [`PageBus`]; each tablet's socket listens to
//! it and passes the request on. Which page shows is the tablet's to keep —
//! two tablets may sit on different pages — so the agent only relays.

use std::sync::{Arc, Mutex, PoisonError};

use flexdeck_plugin::builtin::deck::{PageControl, PageTarget};
use flexdeck_plugin::PluginError;
use flexdeck_protocol::{Config, PageEvent};
use flexdeck_store::{ConfigStore, CONFIG};
use tokio::sync::broadcast;

pub struct PageBus {
    sender: broadcast::Sender<PageEvent>,
    store: Arc<Mutex<ConfigStore>>,
}

impl std::fmt::Debug for PageBus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("PageBus").finish_non_exhaustive()
    }
}

impl PageBus {
    pub fn new(store: Arc<Mutex<ConfigStore>>) -> Self {
        Self {
            sender: broadcast::channel(16).0,
            store,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PageEvent> {
        self.sender.subscribe()
    }
}

impl PageControl for PageBus {
    fn show(&self, target: PageTarget) -> Result<(), PluginError> {
        if self.sender.receiver_count() == 0 {
            return Err(PluginError::Failed("no tablet is connected to show the page".into()));
        }
        let event = match target {
            PageTarget::Page(page_id) => PageEvent { page_id, step: 0 },
            PageTarget::Step(step) => PageEvent { page_id: String::new(), step },
        };
        let _ = self.sender.send(event);
        Ok(())
    }

    /// The first profile's pages: the one a tablet shows.
    fn pages(&self) -> Vec<(String, String)> {
        let snapshot = self
            .store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot_document(&CONFIG)
            .ok()
            .flatten();
        let Some((_, json)) = snapshot else { return Vec::new() };
        let Ok(config) = serde_json::from_str::<Config>(&json) else { return Vec::new() };
        config
            .profiles
            .first()
            .map(|profile| {
                profile
                    .pages
                    .iter()
                    .enumerate()
                    .map(|(index, page)| {
                        // As the tablet names a page without an id.
                        let id = if page.id.is_empty() { format!("page-{}", index + 1) } else { page.id.clone() };
                        (id, page.name.clone())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_reaches_every_listener_and_none_listening_is_a_failure() {
        let store = Arc::new(Mutex::new(ConfigStore::open_in_memory().unwrap()));
        let bus = PageBus::new(Arc::clone(&store));
        assert!(bus.show(PageTarget::Step(1)).is_err(), "nobody to show it");

        let mut tablet = bus.subscribe();
        bus.show(PageTarget::Page("stream".into())).unwrap();
        assert_eq!(tablet.try_recv().unwrap(), PageEvent { page_id: "stream".into(), step: 0 });

        store
            .lock()
            .unwrap()
            .commit_document(
                &CONFIG,
                r#"{"schema":4,"profiles":[{"id":"p","pages":[{"id":"home","name":"Home"},{"name":"Stream"}]}]}"#,
            )
            .unwrap();
        assert_eq!(
            bus.pages(),
            [("home".to_string(), "Home".to_string()), ("page-2".to_string(), "Stream".to_string())]
        );
    }
}
