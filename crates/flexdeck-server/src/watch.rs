use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tokio::time::MissedTickBehavior;

pub const PRESS: Duration = Duration::from_millis(500);
pub const PLUGIN_WORK: Duration = Duration::from_secs(30);
pub const BACKGROUND: Duration = Duration::from_secs(2);
pub const AUTOMATION: Duration = Duration::from_secs(30);

const CHECK_EVERY: Duration = Duration::from_secs(30);
const HEALTH_EVERY_CHECKS: u64 = 10;

struct Entry {
    what: &'static str,
    detail: String,
    started: Instant,
    limit: Duration,
    reported: bool,
}

static NEXT: AtomicU64 = AtomicU64::new(0);
static BUSY: Mutex<BTreeMap<u64, Entry>> = Mutex::new(BTreeMap::new());

fn busy_map() -> std::sync::MutexGuard<'static, BTreeMap<u64, Entry>> {
    BUSY.lock().unwrap_or_else(PoisonError::into_inner)
}

#[must_use]
pub struct Busy {
    id: u64,
}

pub fn busy(what: &'static str, detail: impl Into<String>, limit: Duration) -> Busy {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let entry = Entry { what, detail: detail.into(), started: Instant::now(), limit, reported: false };
    busy_map().insert(id, entry);
    Busy { id }
}

impl Drop for Busy {
    fn drop(&mut self) {
        let Some(entry) = busy_map().remove(&self.id) else { return };
        let took = entry.started.elapsed();
        if took > entry.limit {
            tracing::warn!(what = entry.what, detail = %entry.detail, ms = took.as_millis() as u64, "slow");
        }
    }
}

pub fn running() -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for entry in busy_map().values() {
        *counts.entry(entry.what).or_insert(0) += 1;
    }
    counts
}

pub fn running_details(what: &str) -> Vec<String> {
    busy_map().values().filter(|entry| entry.what == what).map(|entry| entry.detail.clone()).collect()
}

fn overdue() ->Vec<(&'static str, String, Duration)> {
    let mut late = Vec::new();
    for entry in busy_map().values_mut() {
        let age = entry.started.elapsed();
        if !entry.reported && age > entry.limit {
            entry.reported = true;
            late.push((entry.what, entry.detail.clone(), age));
        }
    }
    late
}

fn oldest() -> Option<(&'static str, String, Duration)> {
    busy_map()
        .values()
        .min_by_key(|entry| entry.started)
        .map(|entry| (entry.what, entry.detail.clone(), entry.started.elapsed()))
}

pub fn spawn_health(handle: &tokio::runtime::Handle) {
    let runtime = handle.clone();
    handle.spawn(async move {
        let mut tick = tokio::time::interval(CHECK_EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut checks: u64 = 0;
        loop {
            tick.tick().await;
            for (what, detail, age) in overdue() {
                tracing::warn!(what, detail = %detail, secs = age.as_secs(), "still running");
            }
            if checks.is_multiple_of(HEALTH_EVERY_CHECKS) {
                let metrics = runtime.metrics();
                let busy = running();
                let total: usize = busy.values().sum();
                let oldest = oldest().map(|(what, detail, age)| format!("{what} {detail} {}s", age.as_secs())).unwrap_or_default();
                tracing::info!(
                    workers = metrics.num_workers(),
                    tasks = metrics.num_alive_tasks(),
                    queued = metrics.global_queue_depth(),
                    busy = total,
                    by_kind = ?busy,
                    oldest = %oldest,
                    "health"
                );
            }
            checks += 1;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_is_counted_while_it_runs_and_forgotten_after() {
        let before = running().get("test-count").copied().unwrap_or(0);
        let first = busy("test-count", "a", Duration::from_secs(60));
        let second = busy("test-count", "b", Duration::from_secs(60));
        assert_eq!(running().get("test-count").copied().unwrap_or(0), before + 2);
        drop(first);
        drop(second);
        assert_eq!(running().get("test-count").copied().unwrap_or(0), before);
    }

    #[test]
    fn overdue_work_is_reported_once() {
        let _stuck = busy("test-overdue", "stuck", Duration::ZERO);
        std::thread::sleep(Duration::from_millis(5));
        let first: Vec<_> = overdue().into_iter().filter(|(what, ..)| *what == "test-overdue").collect();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1, "stuck");
        let again = overdue().into_iter().filter(|(what, ..)| *what == "test-overdue").count();
        assert_eq!(again, 0);
    }
}
