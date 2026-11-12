use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rustcord::app::RustcordApp;
use rustcord::settings::Settings;

const NVIDIA_EGL_VENDOR: &str = "/usr/share/glvnd/egl_vendor.d/10_nvidia.json";
const NVIDIA_DEVICE: &str = "/dev/nvidiactl";
const EGL_VENDOR_VAR: &str = "__EGL_VENDOR_LIBRARY_FILENAMES";

/// Decide whether to restrict libglvnd to the NVIDIA EGL vendor.
///
/// When both the NVIDIA vendor description and the kernel device exist, telling
/// libglvnd not to initialise Mesa as well avoids mapping `libLLVM.so` and the
/// gallium driver — measured at roughly 35 MB of resident pages. The override is
/// skipped when the user already set the variable, or on machines without an
/// NVIDIA driver, so AMD/Intel systems keep their normal EGL selection.
fn pick_egl_vendor(
    existing: Option<&OsStr>,
    nvidia_vendor_json: &Path,
    nvidia_device: &Path,
) -> Option<PathBuf> {
    if existing.is_some_and(|value| !value.is_empty()) {
        return None;
    }
    if !nvidia_vendor_json.exists() || !nvidia_device.exists() {
        return None;
    }
    Some(nvidia_vendor_json.to_path_buf())
}

fn maybe_pin_egl_vendor() {
    let existing = std::env::var_os(EGL_VENDOR_VAR);
    if let Some(vendor) = pick_egl_vendor(
        existing.as_deref(),
        Path::new(NVIDIA_EGL_VENDOR),
        Path::new(NVIDIA_DEVICE),
    ) {
        std::env::set_var(EGL_VENDOR_VAR, &vendor);
        log::info!(
            "libglvnd pinned to the NVIDIA EGL vendor ({}); Mesa is not initialised",
            vendor.display()
        );
    }
}

fn main() -> Result<()> {
    // Capture logs both on stderr and in the in-memory ring used by the
    // Diagnostics page.
    rustcord::logbuf::install("info");

    // Must happen before the GL context is created.
    maybe_pin_egl_vendor();

    let mut settings = Settings::load_or_default();
    settings.normalize();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Rustcord"),
        ..Default::default()
    };

    eframe::run_native(
        "Rustcord",
        native_options,
        Box::new(move |cc| Ok(Box::new(RustcordApp::new(cc, settings)))),
    )
    .map_err(|err| anyhow::anyhow!("eframe failed: {err}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        vendor: PathBuf,
        device: PathBuf,
    }

    fn fixture(with_vendor: bool, with_device: bool) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let vendor = dir.path().join("10_nvidia.json");
        let device = dir.path().join("nvidiactl");
        if with_vendor {
            std::fs::write(&vendor, "{}").expect("write vendor");
        }
        if with_device {
            std::fs::write(&device, "").expect("write device");
        }
        Fixture {
            _dir: dir,
            vendor,
            device,
        }
    }

    #[test]
    fn pins_when_nvidia_is_present_and_unset() {
        let fixture = fixture(true, true);
        let picked = pick_egl_vendor(None, &fixture.vendor, &fixture.device);
        assert_eq!(picked, Some(fixture.vendor.clone()));
    }

    #[test]
    fn never_overrides_an_explicit_user_value() {
        let fixture = fixture(true, true);
        let user = OsStr::new("/usr/share/glvnd/egl_vendor.d/50_mesa.json");
        assert_eq!(
            pick_egl_vendor(Some(user), &fixture.vendor, &fixture.device),
            None
        );
    }

    #[test]
    fn empty_env_var_counts_as_unset() {
        let fixture = fixture(true, true);
        let empty = OsStr::new("");
        assert_eq!(
            pick_egl_vendor(Some(empty), &fixture.vendor, &fixture.device),
            Some(fixture.vendor.clone())
        );
    }

    #[test]
    fn skips_without_nvidia_hardware() {
        let fixture = fixture(true, false);
        assert_eq!(
            pick_egl_vendor(None, &fixture.vendor, &fixture.device),
            None
        );
    }

    #[test]
    fn skips_without_the_vendor_description() {
        let fixture = fixture(false, true);
        assert_eq!(
            pick_egl_vendor(None, &fixture.vendor, &fixture.device),
            None
        );
    }
}
