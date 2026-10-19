use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::time::ChronoLocal;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;

pub const KEEP_DAYS: u64 = 7;
pub const DAILY_BYTES: u64 = 50 * 1024 * 1024;
const PREFIX: &str = "flexdeck";
const SUFFIX: &str = "log";
const DEFAULT_FILTER: &str = "warn,flexdeck_server=info,flexdeck_agent=info,flexdeck_desktop=info,flexdeck_plugin_host=info,flexdeck_cloud=info,panic=error";
const FILTER_ENV: &str = "FLEXDECK_LOG";
const DAY: u64 = 24 * 60 * 60;

pub fn init(data_dir: &Path) -> Result<PathBuf, String> {
    let dir = data_dir.join("logs");
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    prune(&dir, SystemTime::now());
    let file = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(PREFIX)
        .filename_suffix(SUFFIX)
        .max_log_files(KEEP_DAYS as usize)
        .build(&dir)
        .map_err(|error| error.to_string())?;
    let filter = std::env::var(FILTER_ENV)
        .ok()
        .and_then(|value| Targets::from_str(&value).ok())
        .unwrap_or_else(|| Targets::from_str(DEFAULT_FILTER).expect("the default filter parses"));
    let sink = Capped::new(file, dir.clone(), DAILY_BYTES);
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(sink)
        .with_ansi(false)
        .with_timer(ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f %:z".to_string()))
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_thread_names(true)
        .with_filter(filter);
    tracing_subscriber::registry().with(layer).try_init().map_err(|error| error.to_string())?;
    log_panics();
    tracing::info!(pid = std::process::id(), dir = %dir.display(), "started");
    Ok(dir)
}

fn log_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|at| format!("{}:{}", at.file(), at.line())).unwrap_or_default();
        let thread = std::thread::current();
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(no message)");
        tracing::error!(target: "panic", thread = thread.name().unwrap_or("unnamed"), %location, "panicked: {message}");
        previous(info);
    }));
}

fn is_log(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(&format!("{PREFIX}.")) && name.ends_with(&format!(".{SUFFIX}")))
}

fn prune(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let cutoff = now.checked_sub(Duration::from_secs(KEEP_DAYS * DAY)).unwrap_or(UNIX_EPOCH);
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_log(&path) {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified());
        if modified.is_ok_and(|at| at < cutoff) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn utc_day(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() / DAY
}

fn written_today(dir: &Path, today: u64) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .filter(|entry| is_log(&entry.path()))
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.modified().is_ok_and(|at| utc_day(at) == today))
        .map(|meta| meta.len())
        .max()
        .unwrap_or(0)
}

struct Budget<W> {
    out: W,
    dir: PathBuf,
    limit: u64,
    day: u64,
    written: u64,
    full: bool,
}

impl<W: Write> Budget<W> {
    fn write_line(&mut self, buf: &[u8], now: SystemTime) -> io::Result<()> {
        let today = utc_day(now);
        if today != self.day {
            self.day = today;
            self.written = 0;
            self.full = false;
            prune(&self.dir, now);
        }
        if self.full {
            return Ok(());
        }
        if self.written + buf.len() as u64 > self.limit {
            self.full = true;
            return self.out.write_all(b"log limit for today reached; nothing more is written until tomorrow (UTC)\n");
        }
        self.out.write_all(buf)?;
        self.written += buf.len() as u64;
        Ok(())
    }
}

struct Capped<W>(Mutex<Budget<W>>);

impl<W: Write> Capped<W> {
    fn new(out: W, dir: PathBuf, limit: u64) -> Self {
        let day = utc_day(SystemTime::now());
        let written = written_today(&dir, day);
        Self(Mutex::new(Budget { out, dir, limit, day, written, full: written >= limit }))
    }
}

struct CappedWriter<'a, W>(&'a Mutex<Budget<W>>);

impl<W: Write> Write for CappedWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).write_line(buf, SystemTime::now())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).out.flush()
    }
}

impl<'a, W: Write + 'a> MakeWriter<'a> for Capped<W> {
    type Writer = CappedWriter<'a, W>;

    fn make_writer(&'a self) -> Self::Writer {
        CappedWriter(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(limit: u64, dir: &Path) -> Budget<Vec<u8>> {
        Budget { out: Vec::new(), dir: dir.to_path_buf(), limit, day: utc_day(SystemTime::now()), written: 0, full: false }
    }

    #[test]
    fn a_day_stops_writing_at_its_limit_and_starts_again_the_next() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = budget(10, dir.path());
        let now = SystemTime::now();
        log.write_line(b"12345\n", now).unwrap();
        log.write_line(b"67890\n", now).unwrap();
        log.write_line(b"extra\n", now).unwrap();
        let text = String::from_utf8(log.out.clone()).unwrap();
        assert!(text.starts_with("12345\nlog limit for today reached"), "{text}");
        assert!(!text.contains("67890") && !text.contains("extra"), "{text}");
        log.write_line(b"next\n", now + Duration::from_secs(DAY)).unwrap();
        assert!(String::from_utf8(log.out).unwrap().ends_with("next\n"));
    }

    #[test]
    fn logs_older_than_a_week_are_removed_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["flexdeck.2026-01-01.log", "flexdeck.2026-01-09.log", "notes.txt"] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let now = SystemTime::now() + Duration::from_secs((KEEP_DAYS + 1) * DAY);
        std::fs::File::options()
            .write(true)
            .open(dir.path().join("flexdeck.2026-01-09.log"))
            .unwrap()
            .set_modified(now - Duration::from_secs(DAY))
            .unwrap();
        prune(dir.path(), now);
        assert!(!dir.path().join("flexdeck.2026-01-01.log").exists());
        assert!(dir.path().join("flexdeck.2026-01-09.log").exists());
        assert!(dir.path().join("notes.txt").exists());
    }
}
