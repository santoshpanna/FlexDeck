//! The page on screen, kept in step between tablets and the configurator.
//!
//! The deck has one page on screen. A tablet reports a page the person turned
//! to there; the configurator and the deck plugin's "go to page" set one. Each
//! is remembered here and sent to every tablet's socket, which passes it on.
//! A tablet does not report a page it was sent, so tablets never echo.
//!
//! Every page sent is named by id: a step ("next page") is resolved here
//! against the page on screen, so two tablets cannot step from different
//! places and land apart.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use flexdeck_plugin::builtin::deck::{PageControl, PageTarget};
use flexdeck_plugin::PluginError;
use flexdeck_protocol::{Config, PageEvent};
use flexdeck_store::{ConfigStore, CONFIG};
use tokio::sync::broadcast;

pub struct PageBus {
    sender: broadcast::Sender<PageEvent>,
    store: Arc<Mutex<ConfigStore>>,
    /// The page on screen; `None` until a tablet or the configurator names one.
    current: Mutex<Option<String>>,
    /// Tablets with a socket open. Not the subscriber count: configurators
    /// listen too, and a page shown only to an editor is shown to nobody.
    tablets: Arc<AtomicUsize>,
}

/// A tablet that can be shown a page, for as long as this is held.
pub struct TabletSeat(Arc<AtomicUsize>);

impl Drop for TabletSeat {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
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
            current: Mutex::new(None),
            tablets: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Count a tablet in until the seat is dropped.
    pub fn seat(&self) -> TabletSeat {
        self.tablets.fetch_add(1, Ordering::SeqCst);
        TabletSeat(Arc::clone(&self.tablets))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PageEvent> {
        self.sender.subscribe()
    }

    /// The page on screen, if one is known.
    pub fn current(&self) -> Option<String> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Show `page_id` everywhere: remembered, and sent to every tablet. Sent
    /// even when unchanged, so a tablet that wandered off is brought back.
    pub fn select(&self, page_id: &str) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Some(page_id.to_string());
        let _ = self.sender.send(PageEvent { page_id: page_id.to_string(), step: 0 });
    }

    /// A tablet now shows `page_id`. Its first report on connecting counts only
    /// while no page is known; afterwards the others follow it.
    pub fn report(&self, page_id: &str, initial: bool) {
        if page_id.is_empty() {
            return;
        }
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if initial && current.is_some() {
            return;
        }
        if current.as_deref() == Some(page_id) {
            return;
        }
        *current = Some(page_id.to_string());
        drop(current);
        let _ = self.sender.send(PageEvent { page_id: page_id.to_string(), step: 0 });
    }
}

impl PageControl for PageBus {
    fn show(&self, target: PageTarget) -> Result<(), PluginError> {
        if self.tablets.load(Ordering::SeqCst) == 0 {
            return Err(PluginError::Failed("no tablet is connected to show the page".into()));
        }
        match target {
            PageTarget::Page(page_id) => self.select(&page_id),
            PageTarget::Step(step) => {
                let pages = self.pages();
                let from = self.current().and_then(|id| pages.iter().position(|(page, _)| *page == id));
                match from {
                    Some(index) if !pages.is_empty() => {
                        let target = (index as i64 + i64::from(step)).rem_euclid(pages.len() as i64);
                        self.select(&pages[target as usize].0);
                    }
                    // No page known yet: each tablet steps from its own.
                    _ => {
                        let _ = self.sender.send(PageEvent { page_id: String::new(), step });
                    }
                }
            }
        }
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
        let _editor = bus.subscribe();
        assert!(bus.show(PageTarget::Step(1)).is_err(), "an editor listening is not a tablet");
        let _seat = bus.seat();
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

    fn two_pages() -> (PageBus, broadcast::Receiver<PageEvent>) {
        let store = Arc::new(Mutex::new(ConfigStore::open_in_memory().unwrap()));
        store
            .lock()
            .unwrap()
            .commit_document(
                &CONFIG,
                r#"{"schema":4,"profiles":[{"id":"p","pages":[{"id":"home","name":"Home"},{"id":"stream","name":"Stream"}]}]}"#,
            )
            .unwrap();
        let bus = PageBus::new(store);
        let tablet = bus.subscribe();
        std::mem::forget(bus.seat());
        (bus, tablet)
    }

    #[test]
    fn a_page_turned_on_a_tablet_is_remembered_and_sent_to_the_others() {
        let (bus, mut tablet) = two_pages();
        bus.report("stream", false);
        assert_eq!(bus.current().as_deref(), Some("stream"));
        assert_eq!(tablet.try_recv().unwrap().page_id, "stream");

        // Already there: nothing to send.
        bus.report("stream", false);
        assert!(tablet.try_recv().is_err());
    }

    #[test]
    fn a_tablet_connecting_does_not_move_a_known_page() {
        let (bus, mut tablet) = two_pages();
        bus.report("home", true);
        assert_eq!(bus.current().as_deref(), Some("home"), "nothing was known: taken");
        let _ = tablet.try_recv();

        bus.select("stream");
        bus.report("home", true);
        assert_eq!(bus.current().as_deref(), Some("stream"));
    }

    #[test]
    fn a_step_is_resolved_from_the_page_on_screen() {
        let (bus, mut tablet) = two_pages();
        bus.select("stream");
        let _ = tablet.try_recv();

        bus.show(PageTarget::Step(1)).unwrap();
        assert_eq!(tablet.try_recv().unwrap(), PageEvent { page_id: "home".into(), step: 0 }, "wraps");
        assert_eq!(bus.current().as_deref(), Some("home"));
    }
}
