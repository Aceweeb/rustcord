//! Linux process/memory sampling from `/proc`.
//!
//! Everything here is read-only and dependency-free: the app measures its own
//! resident set, CPU time, thread count and file descriptors every sample tick,
//! which is what the Memory page and the status bar display. Numbers reported
//! by the UI come from these functions, never from an estimate.
//!
//! Scope note: this is a native single-process renderer (no WebView), so there
//! are no renderer subprocesses to add. `VmHWM` is the kernel's own high-water
//! mark and is what the documentation quotes as peak RSS.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// `USER_HZ` used by `/proc/<pid>/stat` on Linux. It is 100 on every supported
/// architecture (the kernel's timer is fixed for this interface).
pub const CLOCK_TICKS: f64 = 100.0;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcSample {
    pub pid: u32,
    /// `VmRSS` in bytes.
    pub rss_bytes: u64,
    /// `VmHWM` in bytes: the kernel's peak-RSS high-water mark.
    pub peak_rss_bytes: u64,
    pub threads: u32,
    pub fds: u32,
    /// utime + stime, in clock ticks.
    pub cpu_ticks: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcStatus {
    pub rss_bytes: u64,
    pub peak_rss_bytes: u64,
    pub threads: u32,
}

#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub rss_bytes: u64,
}

fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix(key).map(str::trim))
}

/// Parse the interesting lines of `/proc/<pid>/status`.
pub fn parse_status(text: &str) -> ProcStatus {
    let kilobytes = |key: &str| {
        field(text, key)
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
            .map(|kb| kb * 1024)
            .unwrap_or(0)
    };

    ProcStatus {
        rss_bytes: kilobytes("VmRSS:"),
        peak_rss_bytes: kilobytes("VmHWM:"),
        threads: field(text, "Threads:")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0),
    }
}

/// Sum of user + system CPU time from `/proc/<pid>/stat`.
///
/// The command name is wrapped in parentheses and may itself contain spaces or
/// parentheses, so parsing starts after the *last* `)`.
pub fn parse_cpu_ticks(stat: &str) -> Option<u64> {
    let after_comm = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    Some(utime + stime)
}

fn read_fd_count(pid: u32) -> u32 {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .map(|entries| entries.flatten().count() as u32)
        .unwrap_or(0)
}

/// Sample any process. Returns `None` when the process vanished or `/proc` is
/// unavailable (permissions, non-Linux host).
pub fn read_proc_sample(pid: u32) -> Option<ProcSample> {
    let status_text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let status = parse_status(&status_text);
    let stat_text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let cpu_ticks = parse_cpu_ticks(&stat_text).unwrap_or(0);

    Some(ProcSample {
        pid,
        rss_bytes: status.rss_bytes,
        peak_rss_bytes: status.peak_rss_bytes,
        threads: status.threads,
        fds: read_fd_count(pid),
        cpu_ticks,
    })
}

pub fn read_self_sample() -> Option<ProcSample> {
    read_proc_sample(std::process::id())
}

/// `MemAvailable` from `/proc/meminfo`, in bytes.
pub fn system_available_bytes() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    field(&text, "MemAvailable:")
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

/// Read-only scan of the process table, largest RSS first. Used by the Memory
/// page so the user can see what else is resident (e.g. the official client).
pub fn top_memory_processes(limit: usize) -> Vec<ProcessEntry> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        let Ok(status_text) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            continue;
        };
        let status = parse_status(&status_text);
        if status.rss_bytes == 0 {
            continue;
        }
        let name = field(&status_text, "Name:").unwrap_or("?").to_string();
        processes.push(ProcessEntry {
            pid,
            name,
            rss_bytes: status.rss_bytes,
        });
    }

    processes.sort_by_key(|process| std::cmp::Reverse(process.rss_bytes));
    processes.truncate(limit);
    processes
}

/// Converts cumulative tick counts into a CPU percentage between two samples.
#[derive(Debug, Default)]
pub struct CpuMeter {
    previous: Option<(u64, Instant)>,
}

impl CpuMeter {
    /// Feed the newest sample. Returns `None` on the first call (no baseline).
    pub fn update(&mut self, ticks: u64, now: Instant, clock_ticks: f64) -> Option<f32> {
        let percent = self.previous.and_then(|(prev_ticks, prev_at)| {
            let elapsed = now.saturating_duration_since(prev_at).as_secs_f64();
            if elapsed <= 0.0 || ticks < prev_ticks {
                return None;
            }
            let used_seconds = (ticks - prev_ticks) as f64 / clock_ticks;
            Some((used_seconds / elapsed * 100.0) as f32)
        });
        self.previous = Some((ticks, now));
        percent
    }

    pub fn reset(&mut self) {
        self.previous = None;
    }
}

/// Bounded ring of RSS samples; the oldest value is evicted once full, so a
/// long session cannot grow the buffer without bound.
#[derive(Debug)]
pub struct MemoryHistory {
    capacity: usize,
    samples: VecDeque<u64>,
}

impl Default for MemoryHistory {
    fn default() -> Self {
        Self::new(600)
    }
}

impl MemoryHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(2),
            samples: VecDeque::with_capacity(capacity.max(2)),
        }
    }

    pub fn push(&mut self, rss_bytes: u64) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(rss_bytes);
    }

    pub fn latest(&self) -> Option<u64> {
        self.samples.back().copied()
    }

    pub fn peak(&self) -> Option<u64> {
        self.samples.iter().copied().max()
    }

    pub fn average(&self) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }
        let total: u64 = self.samples.iter().sum();
        Some(total / self.samples.len() as u64)
    }

    pub fn samples(&self) -> &VecDeque<u64> {
        &self.samples
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

/// Human-readable binary byte size (`134.2 MB`).
pub fn human_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GB", value / GIB)
    } else if value >= MIB {
        format!("{:.1} MB", value / MIB)
    } else if value >= KIB {
        format!("{:.0} KB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Format a duration since process start as `1h 04m 09s`.
pub fn human_uptime(elapsed: Duration) -> String {
    let total = elapsed.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else {
        format!("{minutes}m {seconds:02}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\trustcord\nState:\tS (sleeping)\nVmPeak:\t 900000 kB\n\
VmHWM:\t  150000 kB\nVmRSS:\t  134200 kB\nThreads:\t4\n";

    #[test]
    fn parses_status_kilobytes_into_bytes() {
        let status = parse_status(STATUS);
        assert_eq!(status.rss_bytes, 134_200 * 1024);
        assert_eq!(status.peak_rss_bytes, 150_000 * 1024);
        assert_eq!(status.threads, 4);
    }

    #[test]
    fn missing_status_fields_are_zero_not_panics() {
        let status = parse_status("Name:\tweird\n");
        assert_eq!(status, ProcStatus::default());
    }

    #[test]
    fn parses_cpu_ticks_after_a_paren_in_the_process_name() {
        // comm contains ") (" (and a space) to prove we split on the last paren.
        let stat = "4321 (weird ) name) S 1 4321 4321 0 -1 4194560 100 0 0 0 11 7 0 0 20 0 1 0 900 1000 5000";
        assert_eq!(parse_cpu_ticks(stat), Some(18));
    }

    #[test]
    fn rejects_malformed_stat() {
        assert_eq!(parse_cpu_ticks("garbage"), None);
        assert_eq!(parse_cpu_ticks("1 (x) S 1"), None);
    }

    #[test]
    fn cpu_meter_needs_two_samples() {
        let mut meter = CpuMeter::default();
        let start = Instant::now();
        assert_eq!(meter.update(100, start, CLOCK_TICKS), None);

        // 5 ticks over 0.5 s with 100 ticks/s => 0.05 s CPU => 10%.
        let percent = meter.update(105, start + Duration::from_millis(500), CLOCK_TICKS);
        assert!((percent.unwrap() - 10.0).abs() < 0.01, "{percent:?}");
    }

    #[test]
    fn cpu_meter_ignores_counter_resets() {
        let mut meter = CpuMeter::default();
        let start = Instant::now();
        meter.update(500, start, CLOCK_TICKS);
        assert_eq!(
            meter.update(10, start + Duration::from_secs(1), CLOCK_TICKS),
            None
        );
    }

    #[test]
    fn memory_history_evicts_oldest_samples() {
        let mut history = MemoryHistory::new(3);
        for value in [10, 20, 30, 40] {
            history.push(value);
        }
        assert_eq!(history.len(), 3);
        assert_eq!(
            history.samples().iter().copied().collect::<Vec<_>>(),
            vec![20, 30, 40]
        );
        assert_eq!(history.latest(), Some(40));
        assert_eq!(history.peak(), Some(40));
        assert_eq!(history.average(), Some(30));
    }

    #[test]
    fn empty_history_reports_nothing() {
        let history = MemoryHistory::default();
        assert!(history.is_empty());
        assert_eq!(history.latest(), None);
        assert_eq!(history.peak(), None);
        assert_eq!(history.average(), None);
    }

    #[test]
    fn human_bytes_uses_binary_units() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2 KB");
        assert_eq!(human_bytes(134_200 * 1024), "131.1 MB");
        assert_eq!(human_bytes(2 * 1024 * 1024 * 1024), "2.00 GB");
    }

    #[test]
    fn human_uptime_formats_minutes_and_hours() {
        assert_eq!(human_uptime(Duration::from_secs(9)), "0m 09s");
        assert_eq!(human_uptime(Duration::from_secs(3849)), "1h 04m 09s");
    }

    /// Reads the real process table. Trivially satisfiable, but it proves the
    /// parser works against a live `/proc` on the supported platform.
    #[test]
    fn samples_the_running_test_process() {
        let sample = read_self_sample().expect("/proc/self readable");
        assert_eq!(sample.pid, std::process::id());
        assert!(sample.rss_bytes > 0);
        assert!(sample.threads >= 1);
        assert!(sample.peak_rss_bytes >= sample.rss_bytes);
    }
}
