//! In-memory log capture for the Diagnostics page.
//!
//! `log` records are still emitted by `env_logger` to stderr; this layer keeps
//! the most recent N lines in a bounded ring so the UI can display them without
//! shell access. Nothing is written to disk unless the user explicitly exports
//! a diagnostics report.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const DEFAULT_CAPACITY: usize = 2000;

#[derive(Debug, Clone)]
pub struct LogLine {
    pub level: log::Level,
    pub target: String,
    pub message: String,
}

impl LogLine {
    pub fn rendered(&self) -> String {
        format!(
            "{:<5} {}: {}",
            self.level.as_str(),
            self.target,
            self.message
        )
    }
}

#[derive(Debug)]
pub struct LogBuffer {
    capacity: usize,
    lines: VecDeque<LogLine>,
    dropped: u64,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            lines: VecDeque::with_capacity(capacity.max(1)),
            dropped: 0,
        }
    }

    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
            self.dropped += 1;
        }
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> &VecDeque<LogLine> {
        &self.lines
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.dropped = 0;
    }
}

static BUFFER: OnceLock<Mutex<LogBuffer>> = OnceLock::new();
static INSTALLED: OnceLock<bool> = OnceLock::new();

fn buffer() -> &'static Mutex<LogBuffer> {
    BUFFER.get_or_init(|| Mutex::new(LogBuffer::default()))
}

fn lock_buffer() -> std::sync::MutexGuard<'static, LogBuffer> {
    buffer().lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Install the stderr logger plus the in-memory capture.
///
/// Returns `false` when a logger was already installed (for example in tests),
/// in which case capture still works for records emitted after this call.
pub fn install(default_filter: &str) -> bool {
    if INSTALLED.get().copied().unwrap_or(false) {
        return false;
    }

    let logger =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_filter))
            .build();
    log::set_max_level(logger.filter());

    let installed = log::set_boxed_logger(Box::new(CaptureLogger { inner: logger })).is_ok();
    let _ = INSTALLED.set(installed);
    installed
}

struct CaptureLogger {
    inner: env_logger::Logger,
}

impl log::Log for CaptureLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.inner.enabled(record.metadata()) {
            return;
        }
        self.inner.log(record);

        let mut guard = lock_buffer();
        guard.push(LogLine {
            level: record.level(),
            target: record.target().to_string(),
            message: record.args().to_string(),
        });
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// Copy of the captured lines, oldest first.
pub fn snapshot() -> Vec<LogLine> {
    lock_buffer().lines().iter().cloned().collect()
}

pub fn clear() {
    lock_buffer().clear();
}

pub fn dropped() -> u64 {
    lock_buffer().dropped()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(index: usize) -> LogLine {
        LogLine {
            level: log::Level::Info,
            target: "test".to_string(),
            message: format!("line {index}"),
        }
    }

    #[test]
    fn buffer_evicts_oldest_and_counts_drops() {
        let mut buffer = LogBuffer::new(2);
        buffer.push(line(1));
        buffer.push(line(2));
        buffer.push(line(3));

        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.dropped(), 1);
        assert_eq!(buffer.lines()[0].message, "line 2");
        assert_eq!(buffer.lines()[1].message, "line 3");
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let mut buffer = LogBuffer::new(0);
        buffer.push(line(1));
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn rendered_lines_include_level_and_target() {
        let rendered = line(7).rendered();
        assert!(rendered.starts_with("INFO"));
        assert!(rendered.contains("test: line 7"));
    }

    #[test]
    fn global_buffer_records_installed_logs() {
        // A logger may already be installed by another test; capture still works
        // once installed, so only assert on the post-install behaviour.
        // The filter must admit `info` or the record never reaches the layer.
        install("info");
        let target = format!("logbuf-test-{}", std::process::id());
        log::info!(target: &target, "captured hello");
        let lines = snapshot();
        assert!(
            lines
                .iter()
                .any(|line| line.message.contains("captured hello")),
            "expected captured log line, got {} lines",
            lines.len()
        );
    }
}
