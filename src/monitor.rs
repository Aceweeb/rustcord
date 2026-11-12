//! Live memory/CPU telemetry for the Memory page and the status bar.
//!
//! Sampling is throttled so an idle window costs nothing measurable: one
//! `/proc` read per second while the settings window is open, one every five
//! seconds otherwise. The ring buffer is bounded, so a long session cannot
//! accumulate samples indefinitely.

use std::time::{Duration, Instant};

use crate::sysinfo::{self, CpuMeter, MemoryHistory, ProcSample, ProcessEntry};

const DEFAULT_HISTORY: usize = 600;
const TOP_PROCESS_MAX_AGE: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct MemoryMonitor {
    pub sample: Option<ProcSample>,
    pub history: MemoryHistory,
    pub cpu: CpuMeter,
    pub cpu_percent: Option<f32>,
    pub last_sample_at: Option<Instant>,
    pub started: Instant,
    pub top: Vec<ProcessEntry>,
    top_at: Option<Instant>,
    pub alerts_fired: u32,
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryMonitor {
    pub fn new() -> Self {
        Self {
            sample: None,
            history: MemoryHistory::new(DEFAULT_HISTORY),
            cpu: CpuMeter::default(),
            cpu_percent: None,
            last_sample_at: None,
            started: Instant::now(),
            top: Vec::new(),
            top_at: None,
            alerts_fired: 0,
        }
    }

    /// Take a sample if `interval` has elapsed. Returns `true` when a new
    /// sample was recorded, which is when the caller should request a repaint.
    pub fn maybe_sample(&mut self, interval: Duration) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_sample_at {
            if now.saturating_duration_since(last) < interval {
                return false;
            }
        }

        let Some(sample) = sysinfo::read_self_sample() else {
            return false;
        };

        self.last_sample_at = Some(now);
        self.history.push(sample.rss_bytes);
        if let Some(percent) = self.cpu.update(sample.cpu_ticks, now, sysinfo::CLOCK_TICKS) {
            self.cpu_percent = Some(percent);
        }
        self.sample = Some(sample);
        true
    }

    pub fn rss_bytes(&self) -> Option<u64> {
        self.sample.map(|sample| sample.rss_bytes)
    }

    pub fn peak_bytes(&self) -> Option<u64> {
        self.sample
            .map(|sample| sample.peak_rss_bytes)
            .or_else(|| self.history.peak())
    }

    /// True when the resident set exceeds the configured budget.
    pub fn over_budget(&self, budget_mb: u32) -> bool {
        let budget = u64::from(budget_mb) * 1024 * 1024;
        self.rss_bytes().is_some_and(|rss| rss > budget)
    }

    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    /// Refresh the (read-only) process table, at most every two seconds.
    pub fn refresh_top(&mut self, limit: usize) -> bool {
        let now = Instant::now();
        if let Some(last) = self.top_at {
            if now.saturating_duration_since(last) < TOP_PROCESS_MAX_AGE {
                return false;
            }
        }
        self.top_at = Some(now);
        self.top = sysinfo::top_memory_processes(limit);
        true
    }

    /// Force the next sample even if the interval has not elapsed.
    pub fn reset(&mut self) {
        self.last_sample_at = None;
        self.cpu.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_always_runs_then_throttles() {
        let mut monitor = MemoryMonitor::new();
        assert!(monitor.maybe_sample(Duration::from_secs(60)));
        assert!(monitor.sample.is_some());
        // A second call inside the interval must be a no-op.
        assert!(!monitor.maybe_sample(Duration::from_secs(60)));
    }

    #[test]
    fn zero_interval_samples_every_call() {
        let mut monitor = MemoryMonitor::new();
        assert!(monitor.maybe_sample(Duration::ZERO));
        assert!(monitor.maybe_sample(Duration::ZERO));
        assert_eq!(monitor.history.len(), 2);
        assert!(monitor.cpu_percent.is_some(), "cpu baseline should exist");
    }

    #[test]
    fn budget_comparison_uses_binary_megabytes() {
        let mut monitor = MemoryMonitor::new();
        monitor.maybe_sample(Duration::ZERO);
        let rss = monitor.rss_bytes().expect("sample");
        let rss_mb = (rss / (1024 * 1024)) as u32;

        assert!(!monitor.over_budget(rss_mb.saturating_add(4)));
        if rss_mb > 1 {
            assert!(monitor.over_budget(rss_mb / 2));
        }
    }

    #[test]
    fn empty_monitor_reports_nothing_and_never_alerts() {
        let monitor = MemoryMonitor::new();
        assert_eq!(monitor.rss_bytes(), None);
        assert_eq!(monitor.peak_bytes(), None);
        assert!(!monitor.over_budget(1));
    }

    #[test]
    fn process_table_refresh_is_throttled() {
        let mut monitor = MemoryMonitor::new();
        assert!(monitor.refresh_top(5));
        assert!(!monitor.refresh_top(5));
        assert!(!monitor.top.is_empty(), "process table should have entries");
    }
}
