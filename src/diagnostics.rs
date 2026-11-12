//! Environment self-checks and the diagnostics report exporter.
//!
//! Checks are read-only probes of the local machine. The exported report
//! contains the app version, OS description, sanitised settings (there are no
//! credentials in `Settings`) and recent log lines. It is written only when the
//! user asks for it, under `~/.local/share/rustcord/diagnostics/`.

use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::logbuf;
use crate::settings::Settings;
use crate::sysinfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Pass => "OK",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

impl Check {
    fn pass(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Pass,
            detail: detail.into(),
        }
    }

    fn warn(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Warn,
            detail: detail.into(),
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Fail,
            detail: detail.into(),
        }
    }
}

/// Probe the local environment. Order is stable so the UI does not jump.
pub fn run_checks(settings: &Settings) -> Vec<Check> {
    let mut checks = Vec::new();

    // Config directory writable.
    let config_dir = Settings::config_dir();
    checks.push(match probe_writable(&config_dir) {
        Ok(()) => Check::pass("Config directory", config_dir.display().to_string()),
        Err(err) => Check::fail("Config directory", format!("{err:#}")),
    });

    // Existing settings file parses.
    let config_path = Settings::config_path();
    if config_path.exists() {
        checks.push(match Settings::load_from(&config_path) {
            Ok(_) => Check::pass("settings.json", "parses and normalises"),
            Err(err) => Check::fail("settings.json", format!("{err:#}")),
        });
    } else {
        checks.push(Check::warn(
            "settings.json",
            "not written yet — defaults are in use",
        ));
    }

    // Theme file.
    match settings.theme_path() {
        Some(path) => {
            let detail = if path.exists() {
                match std::fs::metadata(&path) {
                    Ok(meta) => format!("{} ({} bytes)", path.display(), meta.len()),
                    Err(err) => format!("{} ({err})", path.display()),
                }
            } else {
                format!("{} does not exist", path.display())
            };
            checks.push(if path.exists() {
                Check::pass("Theme file", detail)
            } else {
                Check::warn("Theme file", detail)
            });
        }
        None => checks.push(Check::warn(
            "Theme file",
            "none configured — built-in preset is active",
        )),
    }

    // /proc sampling.
    checks.push(match sysinfo::read_self_sample() {
        Some(sample) => Check::pass(
            "Process sampling",
            format!(
                "RSS {} · peak {} · {} threads",
                sysinfo::human_bytes(sample.rss_bytes),
                sysinfo::human_bytes(sample.peak_rss_bytes),
                sample.threads
            ),
        ),
        None => Check::fail("Process sampling", "/proc is not readable"),
    });

    // Display server.
    let wayland = std::env::var_os("WAYLAND_DISPLAY");
    let x11 = std::env::var_os("DISPLAY");
    checks.push(match (wayland, x11) {
        (Some(display), _) => Check::pass(
            "Display server",
            format!("Wayland ({})", display.to_string_lossy()),
        ),
        (None, Some(display)) => Check::pass(
            "Display server",
            format!("X11 ({})", display.to_string_lossy()),
        ),
        (None, None) => Check::warn("Display server", "neither Wayland nor X11 detected"),
    });

    // Secret Service availability (needed only for bot-token storage).
    checks.push(match std::env::var_os("DBUS_SESSION_BUS_ADDRESS") {
        Some(_) => Check::pass("Secret Service", "session bus available for keyring"),
        None => Check::warn(
            "Secret Service",
            "no DBUS session bus — bot tokens cannot be stored in the keyring",
        ),
    });

    // Log capture.
    let captured = logbuf::snapshot().len();
    checks.push(Check::pass(
        "Log capture",
        format!("{captured} line(s) buffered, {} dropped", logbuf::dropped()),
    ));

    checks
}

/// Write a probe file to prove the directory is actually usable.
fn probe_writable(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let probe = dir.join(".write-probe");
    std::fs::write(&probe, b"ok").with_context(|| format!("writing {}", probe.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn os_description() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("PRETTY_NAME="))
                .map(|value| value.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "unknown Linux".to_string())
}

/// Full text report. Pure formatting, so it is unit-tested.
pub fn render_report(settings: &Settings, checks: &[Check], log_limit: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Rustcord diagnostics report");
    let _ = writeln!(out, "generated: {}", chrono::Utc::now().to_rfc3339());
    let _ = writeln!(out, "version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(out, "os: {}", os_description());
    let _ = writeln!(
        out,
        "session: {} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(
        out,
        "executable: {}",
        std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "?".to_string())
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "== checks ==");
    for check in checks {
        let _ = writeln!(
            out,
            "[{}] {}: {}",
            check.status.symbol(),
            check.name,
            check.detail
        );
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "== system ==");
    if let Some(sample) = sysinfo::read_self_sample() {
        let _ = writeln!(out, "rss: {}", sysinfo::human_bytes(sample.rss_bytes));
        let _ = writeln!(
            out,
            "peak_rss: {}",
            sysinfo::human_bytes(sample.peak_rss_bytes)
        );
        let _ = writeln!(out, "threads: {}", sample.threads);
        let _ = writeln!(out, "fds: {}", sample.fds);
    } else {
        let _ = writeln!(out, "process sampling unavailable");
    }
    if let Some(available) = sysinfo::system_available_bytes() {
        let _ = writeln!(out, "system_available: {}", sysinfo::human_bytes(available));
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "== settings ==");
    let _ = writeln!(out, "{}", settings.diagnostics_json());
    let _ = writeln!(out);

    let _ = writeln!(out, "== recent log ({log_limit} max) ==");
    let lines = logbuf::snapshot();
    let start = lines.len().saturating_sub(log_limit);
    for line in &lines[start..] {
        let _ = writeln!(out, "{}", line.rendered());
    }

    out
}

/// Write a report to the data directory and return its path.
pub fn export(settings: &Settings) -> Result<PathBuf> {
    let checks = run_checks(settings);
    let report = render_report(settings, &checks, 500);

    let dir = Settings::data_dir().join("diagnostics");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("diagnostics-{stamp}.txt"));
    std::fs::write(&path, report).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_contains_version_checks_and_no_secret_fields() {
        let settings = Settings::default();
        let checks = vec![
            Check::pass("Config directory", "/tmp/x"),
            Check::warn("Theme file", "none configured"),
            Check::fail("Process sampling", "/proc unreadable"),
        ];
        let report = render_report(&settings, &checks, 10);

        assert!(report.contains(env!("CARGO_PKG_VERSION")));
        assert!(report.contains("[OK] Config directory"));
        assert!(report.contains("[WARN] Theme file"));
        assert!(report.contains("[FAIL] Process sampling"));
        assert!(report.contains("== settings =="));
        assert!(report.contains("theme_preset"));
        assert!(!report.to_lowercase().contains("bot_token"));
    }

    #[test]
    fn walking_checks_on_this_machine_finds_a_usable_config_dir() {
        let checks = run_checks(&Settings::default());
        let config = checks
            .iter()
            .find(|check| check.name == "Config directory")
            .expect("config dir check");
        assert_eq!(
            config.status,
            CheckStatus::Pass,
            "config dir not writable: {}",
            config.detail
        );
    }

    #[test]
    fn status_symbols_are_distinct() {
        assert_ne!(CheckStatus::Pass.symbol(), CheckStatus::Warn.symbol());
        assert_ne!(CheckStatus::Warn.symbol(), CheckStatus::Fail.symbol());
    }
}
